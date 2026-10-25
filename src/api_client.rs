//! Bounded local control client. The endpoint token never enters URLs or logs.

use crate::{local_api::TorrentView, service_discovery::EndpointLease};
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

    pub async fn pause(&self, id: &str) -> anyhow::Result<()> {
        self.post(id, "pause").await
    }
    pub async fn resume(&self, id: &str) -> anyhow::Result<()> {
        self.post(id, "resume").await
    }
    pub async fn remove(&self, id: &str) -> anyhow::Result<()> {
        self.post(id, "remove").await
    }

    async fn post(&self, id: &str, action: &str) -> anyhow::Result<()> {
        let parsed = uuid::Uuid::parse_str(id).context("invalid torrent ID")?;
        ensure!(parsed.hyphenated().to_string() == id, "invalid torrent ID");
        let response = self
            .http
            .post(format!(
                "http://{}/api/v1/torrents/{id}/{action}",
                self.addr
            ))
            .bearer_auth(&self.token)
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
