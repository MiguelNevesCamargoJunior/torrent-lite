use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use torrent_lite::magnet::Magnet;
use torrent_lite::torrent_file::inspect_torrent;

#[derive(Parser)]
#[command(
    version,
    about = "Offline torrent input inspection (development preview)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
