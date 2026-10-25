//! Loopback-only, bearer-authenticated API. The token stays in process memory.

use crate::{
    service_core::ServiceCore,
    state_store::{DesiredState, SourceKind},
};
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Path, Request, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, HOST, ORIGIN},
    },
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};

const MAX_ADD_BODY_BYTES: usize = 11 * 1024 * 1024;

#[derive(Clone)]
struct ApiState {
    service: Arc<ServiceCore>,
    token: Arc<str>,
    host: Arc<str>,
    origin: Arc<str>,
    add_permits: Arc<Semaphore>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddRequest {
    magnet: Option<String>,
    metainfo_base64: Option<String>,
    #[serde(default)]
    paused: bool,
}

#[derive(Serialize, Deserialize)]
pub struct AddView {
    pub id: String,
    pub info_hash: String,
    pub desired: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TorrentView {
    pub id: String,
    pub info_hash: String,
    pub source_kind: String,
    pub desired: String,
    pub progress_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub finished: Option<bool>,
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
            add_permits: Arc::new(Semaphore::new(2)),
        };
        let router = Router::new()
            .route("/api/v1/torrents", get(list).post(add))
            .route("/api/v1/torrents/{id}/remove", post(remove))
            .route("/api/v1/torrents/{id}/pause", post(pause))
            .route("/api/v1/torrents/{id}/resume", post(resume))
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
            }
            .to_owned(),
            desired: match item.desired {
                DesiredState::Running => "running",
                DesiredState::Paused => "paused",
            }
            .to_owned(),
            progress_bytes: progress.map(|value| value.0),
            total_bytes: progress.map(|value| value.1),
            finished: progress.map(|value| value.2),
        });
    }
    Ok(Json(output))
}

async fn add(
    State(state): State<ApiState>,
    request: Request,
) -> Result<(StatusCode, Json<AddView>), StatusCode> {
    authorize(request.headers(), &state)?;
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if !content_type
        .is_some_and(|value| value == "application/json" || value.starts_with("application/json;"))
    {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let _permit = state
        .add_permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let bytes = tokio::time::timeout(
        Duration::from_secs(10),
        to_bytes(request.into_body(), MAX_ADD_BODY_BYTES),
    )
    .await
    .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
    .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let input: AddRequest = serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let item = match (input.magnet, input.metainfo_base64) {
        (Some(magnet), None) => {
            state
                .service
                .add_magnet_with_pause(&magnet, input.paused)
                .await
        }
        (None, Some(encoded)) => {
            let metainfo = data_encoding::BASE64
                .decode(encoded.as_bytes())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            state.service.add_torrent(&metainfo, input.paused).await
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    }
    .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    Ok((
        StatusCode::CREATED,
        Json(AddView {
            id: item.id,
            info_hash: item.info_hash,
            desired: match item.desired {
                DesiredState::Running => "running",
                DesiredState::Paused => "paused",
            }
            .to_owned(),
        }),
    ))
}

async fn remove(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&headers, &state)?;
    require_id(&state, &id)?;
    state
        .service
        .remove(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

fn require_id(state: &ApiState, id: &str) -> Result<(), StatusCode> {
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
    Ok(())
}

async fn pause(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&headers, &state)?;
    require_id(&state, &id)?;
    state
        .service
        .pause(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resume(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&headers, &state)?;
    require_id(&state, &id)?;
    state
        .service
        .resume(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
