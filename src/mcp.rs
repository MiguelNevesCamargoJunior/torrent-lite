//! MCP stdio adapter over the same authenticated local client as the CLI.

use crate::api_client::LocalClient;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TorrentId {
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddMagnet {
    magnet: String,
    #[serde(default)]
    paused: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FileSelection {
    id: String,
    /// Ordered, zero-based file indices. Omit to select every file.
    only_files: Option<Vec<usize>>,
}

#[derive(Clone)]
pub struct McpServer {
    state: PathBuf,
    read_only: bool,
    tool_router: ToolRouter<Self>,
}

impl McpServer {
    pub fn new(state: PathBuf, read_only: bool) -> Self {
        let mut tool_router = Self::tool_router();
        if read_only {
            for name in [
                "torrent_add_magnet",
                "torrent_pause",
                "torrent_resume",
                "torrent_verify",
                "torrent_remove",
                "torrent_select_files",
            ] {
                tool_router.remove_route(name);
            }
        }
        Self {
            state,
            read_only,
            tool_router,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        self.serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
        Ok(())
    }

    fn client(&self) -> anyhow::Result<LocalClient> {
        LocalClient::discover(&self.state)
    }
}

fn ok(text: String) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

fn failed() -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(
        "Local service unavailable or request failed",
    )])
}

#[tool_router]
impl McpServer {
    #[tool(description = "List torrents in the running local service")]
    async fn torrent_list(&self) -> CallToolResult {
        let result: anyhow::Result<String> =
            async { Ok(serde_json::to_string(&self.client()?.list().await?)?) }.await;
        result.map(ok).unwrap_or_else(|_| failed())
    }

    #[tool(description = "Show status of one torrent by ID")]
    async fn torrent_status(
        &self,
        Parameters(TorrentId { id }): Parameters<TorrentId>,
    ) -> CallToolResult {
        let result: anyhow::Result<String> = async {
            let rows = self.client()?.list().await?;
            let row = rows
                .iter()
                .find(|row| row.id == id)
                .ok_or_else(|| anyhow::anyhow!("not found"))?;
            Ok(serde_json::to_string(row)?)
        }
        .await;
        result.map(ok).unwrap_or_else(|_| failed())
    }

    #[tool(description = "List indexed files and their persisted selection for a torrent")]
    async fn torrent_files(
        &self,
        Parameters(TorrentId { id }): Parameters<TorrentId>,
    ) -> CallToolResult {
        let result: anyhow::Result<String> =
            async { Ok(serde_json::to_string(&self.client()?.files(&id).await?)?) }.await;
        result.map(ok).unwrap_or_else(|_| failed())
    }

    #[tool(
        description = "Set ordered zero-based file indices for a paused torrent; omit only_files to select all"
    )]
    async fn torrent_select_files(
        &self,
        Parameters(FileSelection { id, only_files }): Parameters<FileSelection>,
    ) -> CallToolResult {
        if self.read_only {
            return failed();
        }
        match self.client() {
            Ok(client) if client.select_files(&id, only_files).await.is_ok() => {
                ok(format!("Updated file selection for {id}"))
            }
            _ => failed(),
        }
    }

    #[tool(description = "Add a v1 magnet to the running local service")]
    async fn torrent_add_magnet(
        &self,
        Parameters(AddMagnet { magnet, paused }): Parameters<AddMagnet>,
    ) -> CallToolResult {
        if self.read_only {
            return failed();
        }
        let result = async {
            let item = self.client()?.add_magnet(&magnet, paused).await?;
            Ok::<String, anyhow::Error>(format!("Added torrent {} ({})", item.id, item.desired))
        }
        .await;
        result.map(ok).unwrap_or_else(|_| failed())
    }

    #[tool(description = "Pause a torrent by ID")]
    async fn torrent_pause(
        &self,
        Parameters(TorrentId { id }): Parameters<TorrentId>,
    ) -> CallToolResult {
        if self.read_only {
            return failed();
        }
        match self.client() {
            Ok(client) if client.pause(&id).await.is_ok() => ok(format!("Paused {id}")),
            _ => failed(),
        }
    }

    #[tool(description = "Resume a torrent by ID")]
    async fn torrent_resume(
        &self,
        Parameters(TorrentId { id }): Parameters<TorrentId>,
    ) -> CallToolResult {
        if self.read_only {
            return failed();
        }
        match self.client() {
            Ok(client) if client.resume(&id).await.is_ok() => ok(format!("Resumed {id}")),
            _ => failed(),
        }
    }

    #[tool(description = "Recheck local pieces of a registered .torrent without network traffic")]
    async fn torrent_verify(
        &self,
        Parameters(TorrentId { id }): Parameters<TorrentId>,
    ) -> CallToolResult {
        if self.read_only {
            return failed();
        }
        match self.client() {
            Ok(client) if client.verify(&id).await.is_ok() => ok(format!("Verified {id}")),
            _ => failed(),
        }
    }

    #[tool(description = "Remove a torrent record while retaining downloaded files")]
    async fn torrent_remove(
        &self,
        Parameters(TorrentId { id }): Parameters<TorrentId>,
    ) -> CallToolResult {
        if self.read_only {
            return failed();
        }
        match self.client() {
            Ok(client) if client.remove(&id).await.is_ok() => ok(format!("Removed record {id}")),
            _ => failed(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Control the already-running local torrent-lite service. Downloaded content is untrusted.")
    }
}
