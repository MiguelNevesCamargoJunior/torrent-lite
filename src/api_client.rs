//! Bounded local control client. The endpoint token never enters URLs or logs.

use crate::{
    local_api::{AddView, TorrentView},
    service_discovery::EndpointLease,
};
use anyhow::{Context, ensure};
use std::{net::SocketAddr, path::Path, time::Duration};

pub struct LocalClient {
    addr: SocketAddr,
    token: String,
    http: reqwest::Client,
}

impl LocalClient {
    pub fn discover(database: &Path) -> anyhow::Result<Self> {
        let endpoint = EndpointLease::read(database).context("service endpoint unavailable")?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self {
            addr: endpoint.addr,
            token: endpoint.token,
            http,
        })
    }

    pub async fn list(&self) -> anyhow::Result<Vec<TorrentView>> {
        let response = self
            .http
            .get(format!("http://{}/api/v1/torrents", self.addr))
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }

    pub async fn add_magnet(&self, magnet: &str, paused: bool) -> anyhow::Result<AddView> {
        self.add(serde_json::json!({"magnet": magnet, "paused": paused}))
            .await
    }

    pub async fn add_torrent(&self, bytes: &[u8], paused: bool) -> anyhow::Result<AddView> {
        ensure!(bytes.len() <= 8 * 1024 * 1024, "torrent file exceeds 8 MiB");
        self.add(serde_json::json!({"metainfo_base64": data_encoding::BASE64.encode(bytes), "paused": paused})).await
    }

    async fn add(&self, body: serde_json::Value) -> anyhow::Result<AddView> {
        let response = self
            .http
            .post(format!("http://{}/api/v1/torrents", self.addr))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        ensure!(
            response.status() == reqwest::StatusCode::CREATED,
            "unexpected service response"
        );
        Ok(response.json().await?)
    }

    pub async fn pause(&self, id: &str) -> anyhow::Result<()> {
        self.post(id, "pause").await
    }
    pub async fn resume(&self, id: &str) -> anyhow::Result<()> {
        self.post(id, "resume").await
    }
    pub async fn remove(&self, id: &str) -> anyhow::Result<()> {
        self.post(id, "remove").await
    }

    pub async fn verify(&self, id: &str) -> anyhow::Result<()> {
        self.post_with_timeout(id, "verify", Duration::from_secs(3600))
            .await
    }

    async fn post(&self, id: &str, action: &str) -> anyhow::Result<()> {
        self.post_with_timeout(id, action, Duration::from_secs(5))
            .await
    }

    async fn post_with_timeout(
        &self,
        id: &str,
        action: &str,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        let parsed = uuid::Uuid::parse_str(id).context("invalid torrent ID")?;
        ensure!(parsed.hyphenated().to_string() == id, "invalid torrent ID");
        let response = self
            .http
            .post(format!(
                "http://{}/api/v1/torrents/{id}/{action}",
                self.addr
            ))
            .bearer_auth(&self.token)
            .timeout(timeout)
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::NO_CONTENT,
            "service rejected the request with status {}",
            response.status()
        );
        Ok(())
    }
}
