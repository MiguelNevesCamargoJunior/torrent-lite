use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use torrent_lite::magnet::Magnet;
use torrent_lite::torrent_file::inspect_torrent;

#[derive(Parser)]
#[command(version, about = "Experimental local-first BitTorrent client")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show a read-only local dashboard in your browser.
    #[cfg(feature = "browser-ui")]
    Ui {
        #[arg(long)]
        state: std::path::PathBuf,
    },
    /// Run the MCP stdio server for an already-running local service.
    #[cfg(feature = "mcp")]
    Mcp {
        #[arg(long)]
        state: std::path::PathBuf,
        #[arg(long)]
        read_only: bool,
    },
    /// Add a magnet or .torrent to the running local service.
    #[cfg(feature = "api-client")]
    Add {
        source: String,
        #[arg(long)]
        state: std::path::PathBuf,
        #[arg(long)]
        paused: bool,
    },
    /// List torrents through the running local service.
    #[cfg(feature = "api-client")]
    List {
        #[arg(long)]
        state: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Pause a torrent in the running service.
    #[cfg(feature = "api-client")]
    Pause {
        id: String,
        #[arg(long)]
        state: std::path::PathBuf,
    },
    /// Resume a torrent in the running service.
    #[cfg(feature = "api-client")]
    Resume {
        id: String,
        #[arg(long)]
        state: std::path::PathBuf,
    },
    /// Remove a torrent record while preserving its downloaded files.
    #[cfg(feature = "api-client")]
    Remove {
        id: String,
        #[arg(long)]
        state: std::path::PathBuf,
    },
    /// Recheck local pieces of a registered .torrent without network traffic.
    #[cfg(feature = "api-client")]
    Verify {
        id: String,
        #[arg(long)]
        state: std::path::PathBuf,
    },
    /// Run the local download service and authenticated API in the foreground.
    #[cfg(feature = "local-api")]
    Serve {
        #[arg(long)]
        output: std::path::PathBuf,
        #[arg(long)]
        state: std::path::PathBuf,
        /// Maximum number of incomplete torrents downloading at once.
        #[arg(long, default_value_t = 3)]
        max_active: usize,
    },
    /// Inspect a v1 magnet locally. Does not connect to peers or download data.
    InspectMagnet {
        magnet: String,
        #[arg(long)]
        json: bool,
    },
    /// Inspect a local v1 .torrent file. Does not download data.
    InspectTorrent {
        path: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Download a local v1 .torrent using the experimental persistent service.
    #[cfg(feature = "service-core")]
    DownloadTorrent {
        path: std::path::PathBuf,
        #[arg(long)]
        output: std::path::PathBuf,
        #[arg(long)]
        state: std::path::PathBuf,
    },
    /// Download a v1 magnet with an HTTP(S) tracker using the experimental service.
    #[cfg(feature = "service-core")]
    DownloadMagnet {
        magnet: String,
        #[arg(long)]
        output: std::path::PathBuf,
        #[arg(long)]
        state: std::path::PathBuf,
    },
}

fn main() -> ExitCode {
    let Cli { command } = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.exit()
        }
        Err(_) => {
            // clap diagnostics can include arbitrary arguments, including tracker secrets.
            eprintln!("error: invalid command arguments; run torrent-lite --help");
            return ExitCode::from(2);
        }
    };
    let output = match command {
        #[cfg(feature = "browser-ui")]
        Commands::Ui { state } => return run_ui(state),
        #[cfg(feature = "mcp")]
        Commands::Mcp { state, read_only } => return run_mcp(state, read_only),
        #[cfg(feature = "api-client")]
        Commands::Add {
            source,
            state,
            paused,
        } => return run_client(ClientAction::Add(source, paused), state),
        #[cfg(feature = "api-client")]
        Commands::List { state, json } => return run_client(ClientAction::List(json), state),
        #[cfg(feature = "api-client")]
        Commands::Pause { id, state } => return run_client(ClientAction::Pause(id), state),
        #[cfg(feature = "api-client")]
        Commands::Resume { id, state } => return run_client(ClientAction::Resume(id), state),
        #[cfg(feature = "api-client")]
        Commands::Remove { id, state } => return run_client(ClientAction::Remove(id), state),
        #[cfg(feature = "api-client")]
        Commands::Verify { id, state } => return run_client(ClientAction::Verify(id), state),
        #[cfg(feature = "local-api")]
        Commands::Serve {
            output,
            state,
            max_active,
        } => return run_serve(output, state, max_active),
        #[cfg(feature = "service-core")]
        Commands::DownloadTorrent {
            path,
            output,
            state,
        } => return download_torrent(path, output, state),
        #[cfg(feature = "service-core")]
        Commands::DownloadMagnet {
            magnet,
            output,
            state,
        } => return run_download(DownloadSource::Magnet(magnet), output, state),
        Commands::InspectMagnet { magnet, json } => {
            let value = match Magnet::parse(&magnet) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::from(2);
                }
            };
            if json {
                serde_json::to_string(&value).expect("string-only JSON schema")
            } else {
                let mut text = format!("Info hash: {}", value.info_hash());
                if let Some(name) = &value.display_name {
                    text.push_str(&format!("\nName: {}", name.escape_default()));
                }
                text
            }
        }
        Commands::InspectTorrent { path, json } => {
            let file = match File::open(path) {
                Ok(file) => file,
                Err(_) => {
                    eprintln!("error: unable to open torrent file");
                    return ExitCode::from(2);
                }
            };
            let mut bytes = Vec::new();
            if file
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .is_err()
            {
                eprintln!("error: unable to read torrent file");
                return ExitCode::from(2);
            }
            let value = match inspect_torrent(&bytes) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("error: {error}");
                    return ExitCode::from(2);
                }
            };
            if json {
                serde_json::to_string(&value).expect("summary has only plain values")
            } else {
                format!(
                    "Info hash: {}\nName: {}\nFiles: {}\nBytes: {}",
                    value.info_hash,
                    value.name.escape_default(),
                    value.files.len(),
                    value.total_bytes
                )
            }
        }
    };
    match writeln!(io::stdout().lock(), "{output}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("error: unable to write output");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "browser-ui")]
fn run_ui(state: std::path::PathBuf) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::FAILURE,
    };
    let result = runtime.block_on(async {
        let ui = torrent_lite::browser_ui::BrowserUi::bind(&state).await?;
        println!("Open {}", ui.url());
        tokio::signal::ctrl_c().await?;
        Ok::<(), anyhow::Error>(())
    });
    if result.is_err() {
        eprintln!("error: unable to start local dashboard");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(feature = "mcp")]
fn run_mcp(state: std::path::PathBuf, read_only: bool) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::FAILURE,
    };
    match runtime.block_on(torrent_lite::mcp::McpServer::new(state, read_only).run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("error: MCP stdio transport failed");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "api-client")]
enum ClientAction {
    Add(String, bool),
    List(bool),
    Pause(String),
    Resume(String),
    Remove(String),
    Verify(String),
}

#[cfg(feature = "api-client")]
fn run_client(action: ClientAction, state: std::path::PathBuf) -> ExitCode {
    use torrent_lite::api_client::LocalClient;
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("error: unable to start runtime");
            return ExitCode::FAILURE;
        }
    };
    let result: anyhow::Result<String> = runtime.block_on(async {
        let client = LocalClient::discover(&state)?;
        match action {
            ClientAction::Add(source, paused) => {
                let item = if source.starts_with("magnet:?") {
                    client.add_magnet(&source, paused).await?
                } else {
                    let file = File::open(&source)?;
                    let mut bytes = Vec::new();
                    file.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                    anyhow::ensure!(bytes.len() <= 8 * 1024 * 1024, "torrent file exceeds 8 MiB");
                    client.add_torrent(&bytes, paused).await?
                };
                Ok(format!("Added: {} ({})", item.id, item.desired))
            }
            ClientAction::List(json) => {
                let rows = client.list().await?;
                if json {
                    Ok(serde_json::to_string(&rows)?)
                } else if rows.is_empty() {
                    Ok("No torrents".to_owned())
                } else {
                    Ok(rows
                        .iter()
                        .map(|row| {
                            format!(
                                "{}  {}  {}/{} bytes",
                                row.id,
                                row.desired,
                                row.progress_bytes.unwrap_or(0),
                                row.total_bytes.unwrap_or(0)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            }
            ClientAction::Pause(id) => {
                client.pause(&id).await?;
                Ok(format!("Paused: {id}"))
            }
            ClientAction::Resume(id) => {
                client.resume(&id).await?;
                Ok(format!("Resumed: {id}"))
            }
            ClientAction::Remove(id) => {
                client.remove(&id).await?;
                Ok(format!("Removed record: {id}"))
            }
            ClientAction::Verify(id) => {
                client.verify(&id).await?;
                Ok(format!("Verified: {id}"))
            }
        }
    });
    match result {
        Ok(output) => match writeln!(io::stdout().lock(), "{output}") {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Err(_) => {
            eprintln!("error: local service unavailable or request failed");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "local-api")]
fn run_serve(output: std::path::PathBuf, state: std::path::PathBuf, max_active: usize) -> ExitCode {
    use std::sync::Arc;
    use torrent_lite::{
        local_api::LocalApi, network_policy::DestinationPolicy, service_core::ServiceCore,
        service_discovery::EndpointLease,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("error: unable to start runtime");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async {
        let service = Arc::new(
            ServiceCore::open_with_limit(
                &state,
                &output,
                DestinationPolicy::PublicOnly,
                max_active,
            )
            .await?,
        );
        let api = LocalApi::bind(service.clone()).await?;
        let lease = match EndpointLease::publish(&state, &api) {
            Ok(lease) => lease,
            Err(error) => {
                service.stop().await;
                return Err(anyhow::Error::from(error));
            }
        };
        println!("Service listening on http://{}", api.local_addr());
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.tick().await;
        let mut queue_error_reported = false;
        let signal = loop {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => break signal,
                _ = interval.tick() => {
                    if service.tick_queue().await.is_err() {
                        if !queue_error_reported {
                            eprintln!("error: unable to activate a queued torrent; retrying");
                            queue_error_reported = true;
                        }
                    } else {
                        queue_error_reported = false;
                    }
                }
            }
        };
        drop(lease);
        drop(api);
        service.stop().await;
        signal.map_err(anyhow::Error::from)
    });
    if result.is_err() {
        eprintln!("error: service failed; verify the state and download directories");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(feature = "service-core")]
fn download_torrent(
    path: std::path::PathBuf,
    output: std::path::PathBuf,
    state: std::path::PathBuf,
) -> ExitCode {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => {
            eprintln!("error: unable to open torrent file");
            return ExitCode::from(2);
        }
    };
    let mut bytes = Vec::new();
    if file
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        eprintln!("error: unable to read torrent file");
        return ExitCode::from(2);
    }
    run_download(DownloadSource::Torrent(bytes), output, state)
}

#[cfg(feature = "service-core")]
enum DownloadSource {
    Torrent(Vec<u8>),
    Magnet(String),
}

#[cfg(feature = "service-core")]
fn run_download(
    source: DownloadSource,
    output: std::path::PathBuf,
    state: std::path::PathBuf,
) -> ExitCode {
    use torrent_lite::{network_policy::DestinationPolicy, service_core::ServiceCore};
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("error: unable to start runtime");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async {
        let service = ServiceCore::open(&state, &output, DestinationPolicy::PublicOnly).await?;
        let item = match source {
            DownloadSource::Torrent(bytes) => service.add_torrent(&bytes, false).await?,
            DownloadSource::Magnet(magnet) => service.add_magnet(&magnet).await?,
        };
        let completion = service.wait_completed(&item.id);
        tokio::pin!(completion);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        let result = loop {
            tokio::select! {
                result = &mut completion => break result,
                _ = interval.tick() => {
                    if let Some((done, total, _)) = service.progress(&item.id).await {
                        eprintln!("Progress: {done}/{total} bytes");
                    }
                }
            }
        };
        service.stop().await;
        result.map(|()| item.info_hash)
    });
    match result {
        Ok(hash) => {
            println!("Completed: {hash}");
            ExitCode::SUCCESS
        }
        Err(_) => {
            eprintln!("error: download did not complete; inspect the state and try again");
            ExitCode::FAILURE
        }
    }
}
