use std::net::Ipv4Addr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};
use torrent_lite::{network_policy::DestinationPolicy, policy_proxy::PolicyProxy};

async fn authenticate(
    proxy: &PolicyProxy,
    password_override: Option<&str>,
) -> (TcpStream, [u8; 2]) {
    let mut stream = TcpStream::connect(proxy.local_addr()).await.unwrap();
    stream.write_all(&[5, 1, 2]).await.unwrap();
    let mut method = [0; 2];
    stream.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [5, 2]);
    let url = url::Url::parse(&proxy.proxy_url()).unwrap();
    let user = url.username().as_bytes();
    let password = password_override
        .unwrap_or(url.password().unwrap())
        .as_bytes();
    let mut auth = vec![1, user.len() as u8];
    auth.extend(user);
    auth.push(password.len() as u8);
    auth.extend(password);
    stream.write_all(&auth).await.unwrap();
    let mut reply = [0; 2];
    stream.read_exact(&mut reply).await.unwrap();
    (stream, reply)
}

async fn connect_v4(stream: &mut TcpStream, address: Ipv4Addr, port: u16) -> [u8; 10] {
    let mut request = vec![5, 1, 0, 1];
    request.extend(address.octets());
    request.extend(port.to_be_bytes());
    stream.write_all(&request).await.unwrap();
    let mut reply = [0; 10];
    stream.read_exact(&mut reply).await.unwrap();
    reply
}

async fn connect_domain(stream: &mut TcpStream, domain: &str, port: u16) -> [u8; 10] {
    let mut request = vec![5, 1, 0, 3, domain.len() as u8];
    request.extend(domain.as_bytes());
    request.extend(port.to_be_bytes());
    stream.write_all(&request).await.unwrap();
    let mut reply = [0; 10];
    stream.read_exact(&mut reply).await.unwrap();
    reply
}

#[tokio::test]
async fn public_policy_refuses_private_connect_after_authentication() {
    let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let proxy = PolicyProxy::bind(DestinationPolicy::PublicOnly)
        .await
        .unwrap();
    assert!(proxy.local_addr().ip().is_loopback());
    let (mut client, auth) = authenticate(&proxy, None).await;
    assert_eq!(auth, [1, 0]);
    let reply = connect_v4(
        &mut client,
        Ipv4Addr::LOCALHOST,
        target.local_addr().unwrap().port(),
    )
    .await;
    assert_eq!(reply[0], 5);
    assert_eq!(reply[1], 2);
    assert!(
        timeout(Duration::from_millis(150), target.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn public_policy_checks_dns_result_before_connecting() {
    let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let proxy = PolicyProxy::bind(DestinationPolicy::PublicOnly)
        .await
        .unwrap();
    let (mut client, auth) = authenticate(&proxy, None).await;
    assert_eq!(auth, [1, 0]);
    let reply = connect_domain(
        &mut client,
        "localhost",
        target.local_addr().unwrap().port(),
    )
    .await;
    assert_eq!(reply[1], 2);
    assert!(
        timeout(Duration::from_millis(150), target.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn incorrect_credential_never_reaches_connect() {
    let proxy = PolicyProxy::bind(DestinationPolicy::AllowLan)
        .await
        .unwrap();
    let (mut client, auth) = authenticate(&proxy, Some("wrong")).await;
    assert_eq!(auth, [1, 1]);
    let mut byte = [0];
    assert_eq!(client.read(&mut byte).await.unwrap(), 0);
}

#[tokio::test]
async fn explicit_lan_mode_relays_bytes_to_the_pinned_ip() {
    let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = target.local_addr().unwrap().port();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = target.accept().await.unwrap();
        let mut bytes = [0; 4];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"ping");
        stream.write_all(b"pong").await.unwrap();
    });
    let proxy = PolicyProxy::bind(DestinationPolicy::AllowLan)
        .await
        .unwrap();
    let (mut client, auth) = authenticate(&proxy, None).await;
    assert_eq!(auth, [1, 0]);
    assert_eq!(
        connect_v4(&mut client, Ipv4Addr::LOCALHOST, port).await[1],
        0
    );
    client.write_all(b"ping").await.unwrap();
    let mut response = [0; 4];
    timeout(Duration::from_secs(2), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response, b"pong");
    assert_eq!(proxy.successful_connections(), 1);
    peer.await.unwrap();
}
