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
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, api::TorrentIdOrHash, limits::LimitsConfig,
    storage::StorageFactoryExt,
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

pub struct ServiceCore {
    state: Arc<Mutex<StateStore>>,
    root: SafeRoot,
    root_path: PathBuf,
    session: Arc<Session>,
    proxy: PolicyProxy,
    policy: DestinationPolicy,
    handles: AsyncMutex<HashMap<String, Arc<ManagedTorrent>>>,
    pending_magnets: AsyncMutex<HashMap<String, JoinHandle<anyhow::Result<Vec<u8>>>>>,
    failures: AsyncMutex<HashMap<String, ActivationFailure>>,
    mutation: AsyncMutex<()>,
    max_active_downloads: usize,
}

pub const DEFAULT_MAX_ACTIVE_DOWNLOADS: usize = 3;

#[derive(Debug, Clone, Copy)]
struct ActivationFailure {
    attempts: u32,
    retry_after: Instant,
}

impl ActivationFailure {
    fn next(previous: Option<Self>) -> Self {
        let attempts = previous.map_or(1, |failure| failure.attempts.saturating_add(1));
        let seconds = 1_u64.checked_shl(attempts.min(6)).unwrap_or(60).min(60);
        Self {
            attempts,
            retry_after: Instant::now() + Duration::from_secs(seconds),
        }
    }

    fn retry_after_secs(self) -> u64 {
        let remaining = self.retry_after.saturating_duration_since(Instant::now());
        remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ServiceSettings {
    max_active_downloads: usize,
    download_limit_bps: Option<NonZeroU32>,
    upload_limit_bps: Option<NonZeroU32>,
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            max_active_downloads: DEFAULT_MAX_ACTIVE_DOWNLOADS,
            download_limit_bps: None,
            upload_limit_bps: None,
        }
    }
}

impl ServiceSettings {
    pub fn new(
        max_active_downloads: usize,
        download_limit_kib: Option<u32>,
        upload_limit_kib: Option<u32>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=100).contains(&max_active_downloads),
            "max active downloads must be between 1 and 100"
        );
        fn limit(value: Option<u32>) -> anyhow::Result<Option<NonZeroU32>> {
            value
                .map(|kib| {
                    anyhow::ensure!(kib >= 16, "bandwidth limit must be at least 16 KiB/s");
                    let bps = kib
                        .checked_mul(1024)
                        .ok_or_else(|| anyhow::anyhow!("bandwidth limit exceeds u32 range"))?;
                    Ok(NonZeroU32::new(bps).expect("validated positive bandwidth"))
                })
                .transpose()
        }
        Ok(Self {
            max_active_downloads,
            download_limit_bps: limit(download_limit_kib)?,
            upload_limit_bps: limit(upload_limit_kib)?,
        })
    }

    pub fn download_limit_bps(&self) -> Option<u32> {
        self.download_limit_bps.map(NonZeroU32::get)
    }

    pub fn upload_limit_bps(&self) -> Option<u32> {
        self.upload_limit_bps.map(NonZeroU32::get)
    }
}

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
        Self::open_with_settings(database, download_root, policy, ServiceSettings::default()).await
    }

    pub async fn open_with_limit(
        database: &Path,
        download_root: &Path,
        policy: DestinationPolicy,
        max_active_downloads: usize,
    ) -> anyhow::Result<Self> {
        let settings = ServiceSettings::new(max_active_downloads, None, None)?;
        Self::open_with_settings(database, download_root, policy, settings).await
    }

    pub async fn open_with_settings(
        database: &Path,
        download_root: &Path,
        policy: DestinationPolicy,
        settings: ServiceSettings,
    ) -> anyhow::Result<Self> {
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
                ratelimits: LimitsConfig {
                    download_bps: settings.download_limit_bps,
                    upload_bps: settings.upload_limit_bps,
                },
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
            pending_magnets: AsyncMutex::new(HashMap::new()),
            failures: AsyncMutex::new(HashMap::new()),
            mutation: AsyncMutex::new(()),
            max_active_downloads: settings.max_active_downloads,
        };
        for item in service.list()? {
            anyhow::ensure!(
                item.destination_root == service.root_path.to_string_lossy(),
                "stored destination differs from configured root"
            );
        }
        service.reconcile_locked(false).await?;
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

    pub fn bandwidth_limits_bps(&self) -> (Option<u32>, Option<u32>) {
        let limits = self.session.ratelimits.get_config();
        (
            limits.download_bps.map(NonZeroU32::get),
            limits.upload_bps.map(NonZeroU32::get),
        )
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

    pub async fn activation_failure(&self, id: &str) -> Option<(&'static str, u64)> {
        self.failures
            .lock()
            .await
            .get(id)
            .map(|failure| ("activation_failed", failure.retry_after_secs()))
    }

    pub async fn is_resolving_magnet(&self, id: &str) -> bool {
        self.pending_magnets.lock().await.contains_key(id)
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
            self.reconcile_locked(false).await?;
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
            self.reconcile_locked(false).await?;
        }
        Ok(item)
    }

    pub async fn resume(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .set_intent(id, DesiredState::Running)?;
        self.failures.lock().await.remove(id);
        self.reconcile_locked(false).await
    }

    pub async fn pause(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .set_intent(id, DesiredState::Paused)?;
        self.failures.lock().await.remove(id);
        self.cancel_pending_magnet(id).await;
        if let Some(handle) = self.handles.lock().await.get(id).cloned() {
            self.session.pause(&handle).await?;
        }
        self.reconcile_locked(false).await
    }

    pub async fn set_file_selection(
        &self,
        id: &str,
        indices: Option<Vec<usize>>,
    ) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .validate_file_selection(id, indices.as_deref())?;
        let current_handle = { self.handles.lock().await.get(id).cloned() };
        if let Some(handle) = current_handle {
            self.session
                .delete(TorrentIdOrHash::Id(handle.id()), false)
                .await?;
            self.handles.lock().await.remove(id);
        }
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .set_file_selection(id, indices.as_deref())?;
        self.failures.lock().await.remove(id);
        Ok(())
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
        self.activate_source(&item, AddTorrent::from_bytes(bytes), true)
            .await?;
        let checked_handle = { self.handles.lock().await.get(id).cloned() }
            .ok_or_else(|| anyhow::anyhow!("verification handle unavailable"))?;
        checked_handle.wait_until_initialized().await?;
        self.failures.lock().await.remove(id);
        self.reconcile_locked(false).await
    }

    pub async fn remove(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .remove(id)?;
        self.failures.lock().await.remove(id);
        self.cancel_pending_magnet(id).await;
        if let Some(handle) = self.handles.lock().await.remove(id) {
            // Only resolved .torrent handles enter this map. librqbit 9.0.1's
            // delete path panics if metadata has not been resolved.
            self.session
                .delete(TorrentIdOrHash::Id(handle.id()), false)
                .await?;
        }
        self.reconcile_locked(false).await
    }

    pub async fn wait_completed(&self, id: &str) -> anyhow::Result<()> {
        loop {
            if let Some(handle) = self.handles.lock().await.get(id).cloned() {
                handle.wait_until_completed().await?;
                return Ok(());
            }
            let item = self
                .list()?
                .into_iter()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow::anyhow!("torrent not found"))?;
            anyhow::ensure!(item.desired == DesiredState::Running, "torrent is paused");
            anyhow::ensure!(
                self.activation_failure(id).await.is_none(),
                "torrent activation failed; retry later"
            );
            self.tick_queue().await?;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub async fn stop(&self) {
        let _guard = self.mutation.lock().await;
        for (_, task) in self.pending_magnets.lock().await.drain() {
            task.abort();
        }
        self.session.stop().await;
    }

    /// Reconcile stored intent with the bounded active set. Call periodically to
    /// release slots after downloads finish without requiring a client request.
    pub async fn tick_queue(&self) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        self.reconcile_locked(true).await
    }

    async fn reconcile_locked(&self, resolve_uncached_magnets: bool) -> anyhow::Result<()> {
        let mut active = 0;
        for item in self.list()? {
            if item.desired != DesiredState::Running {
                continue;
            }
            let existing = { self.handles.lock().await.get(&item.id).cloned() };
            if let Some(handle) = existing {
                self.failures.lock().await.remove(&item.id);
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
            } else {
                let pending = {
                    let mut pending = self.pending_magnets.lock().await;
                    if active >= self.max_active_downloads {
                        if let Some(task) = pending.remove(&item.id) {
                            task.abort();
                        }
                        None
                    } else if pending.get(&item.id).is_some_and(JoinHandle::is_finished) {
                        pending.remove(&item.id)
                    } else if pending.contains_key(&item.id) {
                        active += 1;
                        continue;
                    } else {
                        None
                    }
                };
                if active >= self.max_active_downloads {
                    continue;
                }
                if let Some(task) = pending {
                    let result = match task.await {
                        Ok(Ok(bytes)) => self.activate_resolved_magnet(&item, bytes).await,
                        _ => Err(anyhow::anyhow!("magnet metadata resolution failed")),
                    };
                    self.record_activation(&item.id, result.is_ok()).await;
                    if result.is_ok() {
                        active += 1;
                    }
                    continue;
                }
                if item.source_kind == SourceKind::Magnet && !resolve_uncached_magnets {
                    let cached = self
                        .state
                        .lock()
                        .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
                        .load_resolved_metainfo(&item.id)?;
                    if cached.is_none() {
                        continue;
                    }
                }
                let previous = self.failures.lock().await.get(&item.id).copied();
                if previous.is_some_and(|failure| failure.retry_after > Instant::now()) {
                    continue;
                }
                if item.source_kind == SourceKind::Magnet {
                    let cached = self
                        .state
                        .lock()
                        .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
                        .load_resolved_metainfo(&item.id)?;
                    if cached.is_none() {
                        let result = self.start_magnet_resolution(&item).await;
                        self.record_activation(&item.id, result.is_ok()).await;
                        if result.is_ok() {
                            active += 1;
                        }
                        continue;
                    }
                }
                let result = match item.source_kind {
                    SourceKind::Torrent => self.activate_torrent(&item).await,
                    SourceKind::Magnet => self.activate_magnet(&item).await,
                };
                self.record_activation(&item.id, result.is_ok()).await;
                if result.is_ok() {
                    active += 1;
                }
            }
        }
        Ok(())
    }

    async fn record_activation(&self, id: &str, succeeded: bool) {
        let mut failures = self.failures.lock().await;
        if succeeded {
            failures.remove(id);
        } else {
            let previous = failures.get(id).copied();
            failures.insert(id.to_owned(), ActivationFailure::next(previous));
        }
    }

    async fn cancel_pending_magnet(&self, id: &str) {
        if let Some(task) = self.pending_magnets.lock().await.remove(id) {
            task.abort();
        }
    }

    async fn start_magnet_resolution(&self, item: &StoredTorrent) -> anyhow::Result<()> {
        let source = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_source(&item.id)?;
        let magnet = std::str::from_utf8(&source)?;
        let uri = sanitize_magnet_with_policy(magnet, self.policy)?.uri;
        let session = self.session.clone();
        let task = tokio::spawn(async move {
            // librqbit resolves metadata before registering a torrent in list_only
            // mode, so cancellation cannot leave a partial download handle behind.
            let response = tokio::time::timeout(
                Duration::from_secs(30),
                session.add_torrent(
                    AddTorrent::from_url(uri),
                    Some(AddTorrentOptions {
                        list_only: true,
                        ..Default::default()
                    }),
                ),
            )
            .await??;
            match response {
                AddTorrentResponse::ListOnly(result) => Ok(result.torrent_bytes.to_vec()),
                _ => Err(anyhow::anyhow!("engine did not return magnet metadata")),
            }
        });
        self.pending_magnets
            .lock()
            .await
            .insert(item.id.clone(), task);
        Ok(())
    }

    async fn activate_resolved_magnet(
        &self,
        item: &StoredTorrent,
        metainfo: Vec<u8>,
    ) -> anyhow::Result<()> {
        let bytes = sanitize_torrent_with_policy(&metainfo, self.policy)?.bytes;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .store_resolved_metainfo(&item.id, &bytes)?;
        self.activate_bytes(item, bytes).await
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
        let bytes = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_resolved_metainfo(&item.id)?
            .ok_or_else(|| anyhow::anyhow!("magnet metadata has not resolved"))?;
        let bytes = sanitize_torrent_with_policy(&bytes, self.policy)?.bytes;
        self.activate_bytes(item, bytes).await
    }

    async fn activate_bytes(&self, item: &StoredTorrent, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.activate_source(item, AddTorrent::from_bytes(bytes), false)
            .await
    }

    async fn activate_source(
        &self,
        item: &StoredTorrent,
        source: AddTorrent<'_>,
        start_paused: bool,
    ) -> anyhow::Result<()> {
        if self.handles.lock().await.contains_key(&item.id) {
            return Ok(());
        }
        let only_files = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_file_selection(&item.id)?;
        let torrent_root = Arc::new(self.root.subdir(&item.id)?);
        let pending = self.session.add_torrent(
            source,
            Some(AddTorrentOptions {
                paused: start_paused,
                overwrite: true,
                peer_limit: Some(50),
                only_files,
                storage_factory: Some(
                    DurableStorageFactory::new(torrent_root, self.state.clone(), item.id.clone())
                        .boxed(),
                ),
                ..Default::default()
            }),
        );
        let result = pending.await?;
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
