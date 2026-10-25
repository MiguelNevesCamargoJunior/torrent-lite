//! Single-process download controller. Stored intent is committed before engine operations.

use crate::{
    engine_storage::DurableStorageFactory,
    network_input::{sanitize_magnet_with_policy, sanitize_torrent_with_policy},
    network_policy::DestinationPolicy,
    policy_proxy::PolicyProxy,
    safe_storage::SafeRoot,
    state_store::{AddInput, DesiredState, SourceKind, StateStore, StoredTorrent},
};
use librqbit::{
    AddTorrent, AddTorrentOptions, ConnectionOptions, ListenerOptions, ManagedTorrent, Session,
    SessionOptions, api::TorrentIdOrHash, storage::StorageFactoryExt,
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Mutex as AsyncMutex;

pub struct ServiceCore {
    state: Arc<Mutex<StateStore>>,
    root: SafeRoot,
    root_path: PathBuf,
    session: Arc<Session>,
    proxy: PolicyProxy,
    policy: DestinationPolicy,
    handles: AsyncMutex<HashMap<String, Arc<ManagedTorrent>>>,
    mutation: AsyncMutex<()>,
    max_active_downloads: usize,
}

pub const DEFAULT_MAX_ACTIVE_DOWNLOADS: usize = 3;

#[derive(Debug)]
pub struct TorrentMetrics {
    pub name: Option<String>,
    pub observed_state: String,
    pub progress_bytes: u64,
    pub total_bytes: u64,
    pub finished: bool,
    pub uploaded_bytes: u64,
    pub download_bytes_per_sec: Option<u64>,
    pub upload_bytes_per_sec: Option<u64>,
    pub peers: Option<u32>,
    pub eta_secs: Option<u64>,
}

impl ServiceCore {
    pub async fn open(
        database: &Path,
        download_root: &Path,
        policy: DestinationPolicy,
    ) -> anyhow::Result<Self> {
        Self::open_with_limit(
            database,
            download_root,
            policy,
            DEFAULT_MAX_ACTIVE_DOWNLOADS,
        )
        .await
    }

    pub async fn open_with_limit(
        database: &Path,
        download_root: &Path,
        policy: DestinationPolicy,
        max_active_downloads: usize,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=100).contains(&max_active_downloads),
            "max active downloads must be between 1 and 100"
        );
        let root = SafeRoot::open(download_root)?;
        let root_path = download_root.canonicalize()?;
        let state = Arc::new(Mutex::new(StateStore::open(database)?));
        let proxy = PolicyProxy::bind(policy).await?;
        let session = Session::new_with_opts(
            root_path.clone(),
            SessionOptions {
                dht: None,
                disable_local_service_discovery: true,
                fastresume: false,
                persistence: None,
                listen: Some(ListenerOptions {
                    listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                    ipv4_only: true,
                    ..Default::default()
                }),
                connect: Some(ConnectionOptions {
                    proxy_url: Some(proxy.proxy_url()),
                    enable_tcp: false,
                    ..Default::default()
                }),
                peer_limit: Some(50),
                concurrent_init_limit: Some(3),
                ..Default::default()
            },
        )
        .await?;
        let service = Self {
            state,
            root,
            root_path,
            session,
            proxy,
            policy,
            handles: AsyncMutex::new(HashMap::new()),
            mutation: AsyncMutex::new(()),
            max_active_downloads,
        };
        for item in service.list()? {
            anyhow::ensure!(
                item.destination_root == service.root_path.to_string_lossy(),
                "stored destination differs from configured root"
            );
        }
        service.reconcile_locked().await?;
        Ok(service)
    }

    pub fn list(&self) -> anyhow::Result<Vec<StoredTorrent>> {
        Ok(self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .recover()?)
    }

    pub fn proxy_connections(&self) -> u64 {
        self.proxy.successful_connections()
    }

    pub async fn progress(&self, id: &str) -> Option<(u64, u64, bool)> {
        let handle = self.handles.lock().await.get(id).cloned()?;
        let stats = handle.stats();
        Some((stats.progress_bytes, stats.total_bytes, stats.finished))
    }

    pub async fn metrics(&self, id: &str) -> Option<TorrentMetrics> {
        let handle = self.handles.lock().await.get(id).cloned()?;
        let stats = handle.stats();
        let (download_bytes_per_sec, upload_bytes_per_sec, peers, eta_secs) =
            if let Some(live) = &stats.live {
                let down = live.download_speed.as_bytes();
                (
                    Some(down),
                    Some(live.upload_speed.as_bytes()),
                    Some(live.snapshot.peer_stats.live),
                    (down > 0).then(|| {
                        stats
                            .total_bytes
                            .saturating_sub(stats.progress_bytes)
                            .div_ceil(down)
                    }),
                )
            } else {
                (None, None, None, None)
            };
        Some(TorrentMetrics {
            name: handle.name(),
            observed_state: stats.state.to_string(),
            progress_bytes: stats.progress_bytes,
            total_bytes: stats.total_bytes,
            finished: stats.finished,
            uploaded_bytes: stats.uploaded_bytes,
            download_bytes_per_sec,
            upload_bytes_per_sec,
            peers,
            eta_secs,
        })
    }

    pub async fn add_torrent(&self, bytes: &[u8], paused: bool) -> anyhow::Result<StoredTorrent> {
        let _guard = self.mutation.lock().await;
        sanitize_torrent_with_policy(bytes, self.policy)?;
        let item = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .register(AddInput::Torrent(bytes), &self.root_path, paused)?;
        if item.desired == DesiredState::Running {
            self.reconcile_locked().await?;
        }
        Ok(item)
    }

    pub async fn add_magnet(&self, magnet: &str) -> anyhow::Result<StoredTorrent> {
        self.add_magnet_with_pause(magnet, false).await
    }

    pub async fn add_magnet_with_pause(
        &self,
        magnet: &str,
        paused: bool,
    ) -> anyhow::Result<StoredTorrent> {
        let _guard = self.mutation.lock().await;
        let sanitized = sanitize_magnet_with_policy(magnet, self.policy)?;
        anyhow::ensure!(
            url::Url::parse(&sanitized.uri)?
                .query_pairs()
                .any(|(key, _)| key == "tr"),
            "magnet requires an HTTP(S) tracker while DHT is disabled"
        );
        let item = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .register(AddInput::Magnet(magnet), &self.root_path, paused)?;
        if item.desired == DesiredState::Running {
            self.reconcile_locked().await?;
        }
        Ok(item)
    }

    pub async fn resume(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .set_intent(id, DesiredState::Running)?;
        self.reconcile_locked().await
    }

    pub async fn pause(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .set_intent(id, DesiredState::Paused)?;
        if let Some(handle) = self.handles.lock().await.get(id).cloned() {
            self.session.pause(&handle).await?;
        }
        self.reconcile_locked().await
    }

    /// Recheck every piece without network traffic, then restore the stored intent.
    pub async fn verify(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        let item = self
            .list()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| anyhow::anyhow!("torrent not found"))?;
        let source = {
            let store = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("state lock poisoned"))?;
            match item.source_kind {
                SourceKind::Torrent => store.load_source(id)?,
                SourceKind::Magnet => store
                    .load_resolved_metainfo(id)?
                    .ok_or_else(|| anyhow::anyhow!("magnet metadata has not resolved"))?,
            }
        };
        let bytes = sanitize_torrent_with_policy(&source, self.policy)?.bytes;
        let current_handle = { self.handles.lock().await.get(id).cloned() };
        if let Some(handle) = current_handle {
            self.session
                .delete(TorrentIdOrHash::Id(handle.id()), false)
                .await?;
            self.handles.lock().await.remove(id);
        }
        self.activate_source(&item, AddTorrent::from_bytes(bytes), None, true)
            .await?;
        let checked_handle = { self.handles.lock().await.get(id).cloned() }
            .ok_or_else(|| anyhow::anyhow!("verification handle unavailable"))?;
        checked_handle.wait_until_initialized().await?;
        self.reconcile_locked().await
    }

    pub async fn remove(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .remove(id)?;
        if let Some(handle) = self.handles.lock().await.remove(id) {
            // Only resolved .torrent handles enter this map. librqbit 9.0.1's
            // delete path panics if metadata has not been resolved.
            self.session
                .delete(TorrentIdOrHash::Id(handle.id()), false)
                .await?;
        }
        self.reconcile_locked().await
    }

    pub async fn wait_completed(&self, id: &str) -> anyhow::Result<()> {
        let handle = self
            .handles
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("torrent is not active"))?;
        handle.wait_until_completed().await?;
        Ok(())
    }

    pub async fn stop(&self) {
        self.session.stop().await;
    }

    /// Reconcile stored intent with the bounded active set. Call periodically to
    /// release slots after downloads finish without requiring a client request.
    pub async fn tick_queue(&self) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.reconcile_locked().await
    }

    async fn reconcile_locked(&self) -> anyhow::Result<()> {
        let mut active = 0;
        for item in self.list()? {
            if item.desired != DesiredState::Running {
                continue;
            }
            let existing = { self.handles.lock().await.get(&item.id).cloned() };
            if let Some(handle) = existing {
                let stats = handle.stats();
                if stats.finished {
                    if stats.state.to_string() != "paused" {
                        self.session.pause(&handle).await?;
                    }
                    continue;
                }
                if active < self.max_active_downloads {
                    if stats.state.to_string() == "paused" {
                        self.session.unpause(&handle).await?;
                    }
                    active += 1;
                } else if stats.state.to_string() != "paused" {
                    self.session.pause(&handle).await?;
                }
            } else if active < self.max_active_downloads {
                match item.source_kind {
                    SourceKind::Torrent => self.activate_torrent(&item).await?,
                    SourceKind::Magnet => self.activate_magnet(&item).await?,
                }
                active += 1;
            }
        }
        Ok(())
    }

    async fn activate_torrent(&self, item: &StoredTorrent) -> anyhow::Result<()> {
        let source = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_source(&item.id)?;
        let bytes = sanitize_torrent_with_policy(&source, self.policy)?.bytes;
        self.activate_bytes(item, bytes).await
    }

    async fn activate_magnet(&self, item: &StoredTorrent) -> anyhow::Result<()> {
        let cached = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_resolved_metainfo(&item.id)?;
        if let Some(bytes) = cached {
            let bytes = sanitize_torrent_with_policy(&bytes, self.policy)?.bytes;
            return self.activate_bytes(item, bytes).await;
        }
        let source = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_source(&item.id)?;
        let magnet = std::str::from_utf8(&source)?;
        let uri = sanitize_magnet_with_policy(magnet, self.policy)?.uri;
        self.activate_source(
            item,
            AddTorrent::from_url(uri),
            Some(Duration::from_secs(30)),
            false,
        )
        .await
    }

    async fn activate_bytes(&self, item: &StoredTorrent, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.activate_source(item, AddTorrent::from_bytes(bytes), None, false)
            .await
    }

    async fn activate_source(
        &self,
        item: &StoredTorrent,
        source: AddTorrent<'_>,
        deadline: Option<Duration>,
        start_paused: bool,
    ) -> anyhow::Result<()> {
        if self.handles.lock().await.contains_key(&item.id) {
            return Ok(());
        }
        let torrent_root = Arc::new(self.root.subdir(&item.id)?);
        let pending = self.session.add_torrent(
            source,
            Some(AddTorrentOptions {
                paused: start_paused,
                overwrite: true,
                peer_limit: Some(50),
                storage_factory: Some(
                    DurableStorageFactory::new(torrent_root, self.state.clone(), item.id.clone())
                        .boxed(),
                ),
                ..Default::default()
            }),
        );
        let result = match deadline {
            Some(deadline) => tokio::time::timeout(deadline, pending).await??,
            None => pending.await?,
        };
        let handle = result
            .into_handle()
            .ok_or_else(|| anyhow::anyhow!("engine did not return a torrent handle"))?;
        if item.source_kind == SourceKind::Magnet {
            let metainfo = handle.with_metadata(|metadata| metadata.torrent_bytes.to_vec())?;
            let validated = sanitize_torrent_with_policy(&metainfo, self.policy);
            let stored = match validated {
                Ok(value) => self
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
                    .store_resolved_metainfo(&item.id, &value.bytes)
                    .map_err(anyhow::Error::from),
                Err(error) => Err(anyhow::Error::from(error)),
            };
            if let Err(error) = stored {
                self.session
                    .delete(TorrentIdOrHash::Id(handle.id()), false)
                    .await?;
                return Err(error);
            }
        }
        self.handles.lock().await.insert(item.id.clone(), handle);
        Ok(())
    }
}
