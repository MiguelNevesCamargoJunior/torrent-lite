//! Single-process download controller. Stored intent is committed before engine operations.

use crate::{
    engine_storage::DurableStorageFactory,
    network_input::sanitize_torrent_with_policy,
    network_policy::DestinationPolicy,
    policy_proxy::PolicyProxy,
    safe_storage::SafeRoot,
    state_store::{AddInput, DesiredState, SourceKind, StateStore, StoredTorrent},
};
use librqbit::{
    AddTorrent, AddTorrentOptions, ConnectionOptions, ListenerOptions, ManagedTorrent, Session,
    SessionOptions, storage::StorageFactoryExt,
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
}

impl ServiceCore {
    pub async fn open(
        database: &Path,
        download_root: &Path,
        policy: DestinationPolicy,
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
        };
        for item in service.list()? {
            anyhow::ensure!(
                item.destination_root == service.root_path.to_string_lossy(),
                "stored destination differs from configured root"
            );
            if item.desired == DesiredState::Running && item.source_kind == SourceKind::Torrent {
                service.activate_torrent(&item).await?;
            }
        }
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

    pub async fn add_torrent(&self, bytes: &[u8], paused: bool) -> anyhow::Result<StoredTorrent> {
        let _guard = self.mutation.lock().await;
        let sanitized = sanitize_torrent_with_policy(bytes, self.policy)?;
        let item = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .register(AddInput::Torrent(bytes), &self.root_path, paused)?;
        if item.desired == DesiredState::Running {
            self.activate_bytes(&item, sanitized.bytes).await?;
        }
        Ok(item)
    }

    pub async fn resume(&self, id: &str) -> anyhow::Result<()> {
        let _guard = self.mutation.lock().await;
        let item = self
            .list()?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| anyhow::anyhow!("torrent not found"))?;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .set_intent(id, DesiredState::Running)?;
        if let Some(handle) = self.handles.lock().await.get(id).cloned() {
            self.session.unpause(&handle).await?;
        } else if item.source_kind == SourceKind::Torrent {
            self.activate_torrent(&item).await?;
        } else {
            anyhow::bail!("magnet activation is not implemented yet");
        }
        Ok(())
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
        Ok(())
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

    async fn activate_torrent(&self, item: &StoredTorrent) -> anyhow::Result<()> {
        let source = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
            .load_source(&item.id)?;
        let bytes = sanitize_torrent_with_policy(&source, self.policy)?.bytes;
        self.activate_bytes(item, bytes).await
    }

    async fn activate_bytes(&self, item: &StoredTorrent, bytes: Vec<u8>) -> anyhow::Result<()> {
        if self.handles.lock().await.contains_key(&item.id) {
            return Ok(());
        }
        let torrent_root = Arc::new(self.root.subdir(&item.id)?);
        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions {
                    overwrite: true,
                    peer_limit: Some(50),
                    storage_factory: Some(
                        DurableStorageFactory::new(
                            torrent_root,
                            self.state.clone(),
                            item.id.clone(),
                        )
                        .boxed(),
                    ),
                    ..Default::default()
                }),
            )
            .await?
            .into_handle()
            .ok_or_else(|| anyhow::anyhow!("engine did not return a torrent handle"))?;
        self.handles.lock().await.insert(item.id.clone(), handle);
        Ok(())
    }
}
