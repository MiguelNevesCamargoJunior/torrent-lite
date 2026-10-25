//! Loopback-only, bearer-authenticated API. The token stays in process memory.

use crate::{
    service_core::ServiceCore,
    state_store::{DesiredState, SourceKind},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, HOST, ORIGIN},
    },
    routing::{get, post},
};
use serde::Serialize;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Clone)]
struct ApiState {
    service: Arc<ServiceCore>,
    token: Arc<str>,
    host: Arc<str>,
    origin: Arc<str>,
}

#[derive(Serialize)]
struct TorrentView {
    id: String,
    info_hash: String,
    source_kind: &'static str,
    desired: &'static str,
    progress_bytes: Option<u64>,
    total_bytes: Option<u64>,
    finished: Option<bool>,
}

pub struct LocalApi {
    addr: SocketAddr,
    token: String,
    task: JoinHandle<()>,
}

impl LocalApi {
    pub async fn bind(service: Arc<ServiceCore>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let mut secret = [0_u8; 32];
        getrandom::fill(&mut secret).map_err(|_| io::Error::other("OS randomness unavailable"))?;
        let token = secret
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let state = ApiState {
            service,
            token: Arc::from(token.as_str()),
            host: Arc::from(addr.to_string()),
            origin: Arc::from(format!("http://{addr}")),
        };
        let router = Router::new()
            .route("/api/v1/torrents", get(list))
            .route("/api/v1/torrents/{id}/remove", post(remove))
            .with_state(state);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self { addr, token, task })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Only the process owner should receive this secret. Never log it or place it in a URL.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for LocalApi {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn constant_time_equal(actual: &[u8], expected: &[u8]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

fn authorize(headers: &HeaderMap, state: &ApiState) -> Result<(), StatusCode> {
    let host = headers.get(HOST).and_then(|value| value.to_str().ok());
    if host != Some(state.host.as_ref()) {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(origin) = headers.get(ORIGIN)
        && origin.to_str().ok() != Some(state.origin.as_ref())
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let value = headers
        .get(AUTHORIZATION)
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    let mut expected = b"Bearer ".to_vec();
    expected.extend_from_slice(state.token.as_bytes());
    if !constant_time_equal(value, &expected) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

async fn list(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TorrentView>>, StatusCode> {
    authorize(&headers, &state)?;
    let records = state
        .service
        .list()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut output = Vec::with_capacity(records.len());
    for item in records {
        let progress = state.service.progress(&item.id).await;
        output.push(TorrentView {
            id: item.id,
            info_hash: item.info_hash,
            source_kind: match item.source_kind {
                SourceKind::Torrent => "torrent",
                SourceKind::Magnet => "magnet",
            },
            desired: match item.desired {
                DesiredState::Running => "running",
                DesiredState::Paused => "paused",
            },
            progress_bytes: progress.map(|value| value.0),
            total_bytes: progress.map(|value| value.1),
            finished: progress.map(|value| value.2),
        });
    }
    Ok(Json(output))
}

async fn remove(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&headers, &state)?;
    if id.len() != 36
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        return Err(StatusCode::NOT_FOUND);
    }
    let exists = state
        .service
        .list()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .iter()
        .any(|item| item.id == id);
    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }
    state
        .service
        .remove(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
