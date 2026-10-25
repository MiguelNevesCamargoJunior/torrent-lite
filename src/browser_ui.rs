//! Ephemeral browser dashboard over the authenticated local client.

use crate::api_client::LocalClient;
use crate::local_api::AddView;
use crate::local_api::FilesView;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path as RoutePath, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CONTENT_TYPE, HOST, ORIGIN},
    },
    response::Html,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::Arc,
};
use tokio::{net::TcpListener, task::JoinHandle};

const DASHBOARD: &str = include_str!("dashboard.html");
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const MAX_MAGNET_BODY: usize = 16 * 1024;
const MAX_TORRENT_BODY: usize = 8 * 1024 * 1024;

#[derive(Serialize)]
struct CsrfView {
    token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MagnetRequest {
    magnet: String,
    #[serde(default)]
    paused: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSelectionRequest {
    only_files: Option<Vec<usize>>,
}

#[derive(Clone)]
struct UiState {
    client: Arc<LocalClient>,
    secret: Arc<str>,
    csrf: Arc<str>,
    host: Arc<str>,
    origin: Arc<str>,
}

pub struct BrowserUi {
    addr: SocketAddr,
    path: String,
    task: JoinHandle<()>,
}

impl BrowserUi {
    pub async fn bind(database: &Path) -> anyhow::Result<Self> {
        let client = Arc::new(LocalClient::discover(database)?);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| io::Error::other("OS randomness unavailable"))?;
        let secret = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut csrf_random = [0_u8; 32];
        getrandom::fill(&mut csrf_random)
            .map_err(|_| io::Error::other("OS randomness unavailable"))?;
        let csrf = csrf_random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let state = UiState {
            client,
            secret: Arc::from(secret.as_str()),
            csrf: Arc::from(csrf.as_str()),
            host: Arc::from(addr.to_string()),
            origin: Arc::from(format!("http://{addr}")),
        };
        let router = Router::new()
            .route("/{key}/", get(page))
            .route("/{key}/data", get(data))
            .route("/{key}/csrf", get(csrf_token))
            .route("/{key}/stop", post(stop_service))
            .route("/{key}/add-magnet", post(add_magnet))
            .route("/{key}/add-torrent", post(add_torrent))
            .route("/{key}/torrents/{id}/{action}", post(mutate_torrent))
            .route("/{key}/torrents/{id}/files", get(files).post(select_files))
            .with_state(state);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self {
            addr,
            path: format!("/{secret}/"),
            task,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
    pub fn page_path(&self) -> &str {
        &self.path
    }
    pub fn url(&self) -> String {
        format!("http://{}{}", self.addr, self.path)
    }
}

impl Drop for BrowserUi {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn check(headers: &HeaderMap, key: &str, state: &UiState) -> Result<(), StatusCode> {
    if key != state.secret.as_ref() {
        return Err(StatusCode::NOT_FOUND);
    }
    if headers.get(HOST).and_then(|value| value.to_str().ok()) != Some(state.host.as_ref()) {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(origin) = headers.get(ORIGIN)
        && origin.to_str().ok() != Some(state.origin.as_ref())
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

fn response_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("content-security-policy", HeaderValue::from_static(CSP));
    headers
}

async fn page(
    State(state): State<UiState>,
    RoutePath(key): RoutePath<String>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Html<&'static str>), StatusCode> {
    check(&headers, &key, &state)?;
    Ok((response_headers(), Html(DASHBOARD)))
}

async fn data(
    State(state): State<UiState>,
    RoutePath(key): RoutePath<String>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<Vec<crate::local_api::TorrentView>>), StatusCode> {
    check(&headers, &key, &state)?;
    let rows = state
        .client
        .list()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok((response_headers(), Json(rows)))
}

async fn csrf_token(
    State(state): State<UiState>,
    RoutePath(key): RoutePath<String>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<CsrfView>), StatusCode> {
    check(&headers, &key, &state)?;
    Ok((
        response_headers(),
        Json(CsrfView {
            token: state.csrf.to_string(),
        }),
    ))
}

fn constant_time_equal(actual: &[u8], expected: &[u8]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

fn check_mutation(
    headers: &HeaderMap,
    key: &str,
    state: &UiState,
    content_type: &str,
) -> Result<(), StatusCode> {
    check(headers, key, state)?;
    if headers.get(ORIGIN).and_then(|value| value.to_str().ok()) != Some(state.origin.as_ref()) {
        return Err(StatusCode::FORBIDDEN);
    }
    let token = headers
        .get("x-csrf-token")
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    if !constant_time_equal(token, state.csrf.as_bytes()) {
        return Err(StatusCode::FORBIDDEN);
    }
    if headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some(content_type)
    {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    Ok(())
}

async fn stop_service(
    State(state): State<UiState>,
    RoutePath(key): RoutePath<String>,
    request: Request,
) -> Result<(HeaderMap, StatusCode), StatusCode> {
    check_mutation(request.headers(), &key, &state, "application/json")?;
    let bytes = to_bytes(request.into_body(), 64)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    if bytes.as_ref() != b"{}" {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .client
        .stop_service()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((response_headers(), StatusCode::NO_CONTENT))
}

async fn add_magnet(
    State(state): State<UiState>,
    RoutePath(key): RoutePath<String>,
    request: Request,
) -> Result<(StatusCode, HeaderMap, Json<AddView>), StatusCode> {
    check_mutation(request.headers(), &key, &state, "application/json")?;
    let bytes = to_bytes(request.into_body(), MAX_MAGNET_BODY)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let body: MagnetRequest =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let item = state
        .client
        .add_magnet(&body.magnet, body.paused)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((StatusCode::CREATED, response_headers(), Json(item)))
}

async fn add_torrent(
    State(state): State<UiState>,
    RoutePath(key): RoutePath<String>,
    request: Request,
) -> Result<(StatusCode, HeaderMap, Json<AddView>), StatusCode> {
    check_mutation(request.headers(), &key, &state, "application/x-bittorrent")?;
    let bytes = to_bytes(request.into_body(), MAX_TORRENT_BODY)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    if bytes.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let item = state
        .client
        .add_torrent(&bytes, false)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((StatusCode::CREATED, response_headers(), Json(item)))
}

async fn files(
    State(state): State<UiState>,
    RoutePath((key, id)): RoutePath<(String, String)>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<FilesView>), StatusCode> {
    check(&headers, &key, &state)?;
    let view = state
        .client
        .files(&id)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((response_headers(), Json(view)))
}

async fn select_files(
    State(state): State<UiState>,
    RoutePath((key, id)): RoutePath<(String, String)>,
    request: Request,
) -> Result<(HeaderMap, StatusCode), StatusCode> {
    check_mutation(request.headers(), &key, &state, "application/json")?;
    let bytes = to_bytes(request.into_body(), 60_000)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let body: FileSelectionRequest =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    state
        .client
        .select_files(&id, body.only_files)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((response_headers(), StatusCode::NO_CONTENT))
}

async fn mutate_torrent(
    State(state): State<UiState>,
    RoutePath((key, id, action)): RoutePath<(String, String, String)>,
    request: Request,
) -> Result<(HeaderMap, StatusCode), StatusCode> {
    check_mutation(request.headers(), &key, &state, "application/json")?;
    let bytes = to_bytes(request.into_body(), 64)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    if bytes.as_ref() != b"{}" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let id_uuid = uuid::Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if id_uuid.hyphenated().to_string() != id {
        return Err(StatusCode::BAD_REQUEST);
    }
    let result = match action.as_str() {
        "pause" => state.client.pause(&id).await,
        "resume" => state.client.resume(&id).await,
        "verify" => state.client.verify(&id).await,
        "remove" => state.client.remove(&id).await,
        _ => return Err(StatusCode::NOT_FOUND),
    };
    result.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((response_headers(), StatusCode::NO_CONTENT))
}
