//! Ephemeral, read-only browser dashboard over the authenticated local client.

use crate::api_client::LocalClient;
use axum::{
    Json, Router,
    extract::{Path as RoutePath, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{HOST, ORIGIN},
    },
    response::Html,
    routing::get,
};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::Arc,
};
use tokio::{net::TcpListener, task::JoinHandle};

const DASHBOARD: &str = include_str!("dashboard.html");
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Clone)]
struct UiState {
    client: Arc<LocalClient>,
    secret: Arc<str>,
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
        let state = UiState {
            client,
            secret: Arc::from(secret.as_str()),
            host: Arc::from(addr.to_string()),
            origin: Arc::from(format!("http://{addr}")),
        };
        let router = Router::new()
            .route("/{key}/", get(page))
            .route("/{key}/data", get(data))
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
