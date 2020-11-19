//! A custom kubelet backend that can run [waSCC](https://wascc.dev/) based workloads
//!
//! The crate provides the [`WasccProvider`] type which can be used
//! as a provider with [`kubelet`].
//!
//! # Example
//! ```rust,no_run
//! use kubelet::{Kubelet, config::Config};
//! use kubelet::store::oci::FileStore;
//! use std::sync::Arc;
//! use wascc_provider::WasccProvider;
//!
//! async fn start() {
//!     // Get a configuration for the Kubelet
//!     let kubelet_config = Config::default();
//!     let client = oci_distribution::Client::default();
//!     let store = Arc::new(FileStore::new(client, &std::path::PathBuf::from("")));
//!
//!     // Load a kubernetes configuration
//!     let kubeconfig = kube::Config::infer().await.unwrap();
//!
//!     // Instantiate the provider type
//!     let provider = WasccProvider::new(store, &kubelet_config, kubeconfig.clone()).await.unwrap();
//!
//!     // Instantiate the Kubelet
//!     let kubelet = Kubelet::new(provider, kubeconfig, kubelet_config).await.unwrap();
//!     // Start the Kubelet and block on it
//!     kubelet.start().await.unwrap();
//! }
//! ```

#![deny(missing_docs)]

use async_trait::async_trait;
use kubelet::backoff::ExponentialBackoffStrategy;
use kubelet::container::Handle as ContainerHandle;
use kubelet::handle::StopHandler;
use kubelet::node::Builder;
use kubelet::pod::{Handle, Pod, PodKey};
use kubelet::provider::Provider;
use kubelet::provider::ProviderError;
use kubelet::store::Store;

use kubelet::volume::Ref;
use log::{debug, info};
use tempfile::NamedTempFile;
use tokio::sync::{RwLock, Notify};
use wascc_fs::FileSystemProvider;
use wascc_host::{Actor, Host, NativeCapability};
use wascc_httpsrv::HttpServerProvider;
use wascc_logging::{LoggingProvider, LOG_PATH_KEY};

extern crate rand;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;

mod states;
use states::registered::Registered;
use states::terminated::Terminated;

/// The architecture that the pod targets.
const TARGET_WASM32_WASCC: &str = "wasm32-wascc";

/// The name of the Filesystem capability.
const FS_CAPABILITY: &str = "wascc:blobstore";

/// The name of the HTTP capability.
const HTTP_CAPABILITY: &str = "wascc:http_server";

/// The name of the Logging capability.
const LOG_CAPABILITY: &str = "wascc:logging";

/// The root directory of waSCC logs.
const LOG_DIR_NAME: &str = "wascc-logs";

/// The key used to define the root directory of the Filesystem capability.
const FS_CONFIG_ROOTDIR: &str = "ROOT";

/// The root directory of waSCC volumes.
const VOLUME_DIR: &str = "volumes";

/// Kubernetes' view of environment variables is an unordered map of string to string.
type EnvVars = std::collections::HashMap<String, String>;

/// A [kubelet::handle::Handle] implementation for a wascc actor
pub struct ActorHandle {
    /// The public key of the wascc Actor that will be stopped
    pub key: String,
    host: Arc<Mutex<Host>>,
    volumes: Vec<VolumeBinding>,
    capabilities: Vec<String>,
}

#[async_trait::async_trait]
impl StopHandler for ActorHandle {
    async fn stop(&mut self) -> anyhow::Result<()> {
        debug!("stopping wascc instance {}", self.key);
        let host = self.host.clone();
        let key = self.key.clone();
        let volumes: Vec<VolumeBinding> = self.volumes.drain(0..).collect();
        let capabilities = self.capabilities.clone();
        tokio::task::spawn_blocking(move || {
            let lock = host.lock().unwrap();
            lock.remove_actor(&key)
                .map_err(|e| anyhow::anyhow!("unable to remove actor: {:?}", e))?;

            if capabilities.contains(&FS_CAPABILITY.to_owned()) {
                for volume in volumes.into_iter() {
                    lock.remove_native_capability(FS_CAPABILITY, Some(volume.name.clone()))
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "unable to remove volume {:?} capability: {:?}",
                                volume.name,
                                e
                            )
                        })?;
                }
            }
            Ok(())
        })
        .await?
    }

    async fn wait(&mut self) -> anyhow::Result<()> {
        // TODO: Figure out if there is a way to wait for an actor to be removed
        Ok(())
    }
}

/// WasccProvider provides a Kubelet runtime implementation that executes WASM binaries.
///
/// Currently, this runtime uses WASCC as a host, loading the primary container as an actor.
/// TODO: In the future, we will look at loading capabilities using the "sidecar" metaphor
/// from Kubernetes.
#[derive(Clone)]
pub struct WasccProvider {
    shared: SharedPodState,
}

#[derive(Clone)]
struct SharedPodState {
    client: kube::Client,
    handles: Arc<RwLock<BTreeMap<PodKey, Handle<ActorHandle, LogHandleFactory>>>>,
    store: Arc<dyn Store + Sync + Send>,
    volume_path: PathBuf,
    log_path: PathBuf,
    host: Arc<Mutex<Host>>,
    port_map: Arc<TokioMutex<BTreeMap<u16, PodKey>>>,
}

impl WasccProvider {
    /// Returns a new wasCC provider configured to use the proper data directory
    /// (including creating it if necessary)
    pub async fn new(
        store: Arc<dyn Store + Sync + Send>,
        config: &kubelet::config::Config,
        kubeconfig: kube::Config,
    ) -> anyhow::Result<Self> {
        let client = kube::Client::new(kubeconfig);
        let host = Arc::new(Mutex::new(Host::new()));
        let log_path = config.data_dir.join(LOG_DIR_NAME);
        let volume_path = config.data_dir.join(VOLUME_DIR);
        let port_map = Arc::new(TokioMutex::new(BTreeMap::<u16, PodKey>::new()));
        tokio::fs::create_dir_all(&log_path).await?;
        tokio::fs::create_dir_all(&volume_path).await?;

        // wascc has native and portable capabilities.
        //
        // Native capabilities are either dynamic libraries (.so, .dylib, .dll)
        // or statically linked Rust libaries. If the native capabilty is a dynamic
        // library it must be loaded and configured through [`NativeCapability::from_file`].
        // If it is a statically linked libary it can be configured through
        // [`NativeCapability::from_instance`].
        //
        // Portable capabilities are WASM modules.  Portable capabilities
        // don't fully work, and won't until the WASI spec has matured.
        //
        // Here we are using the native capabilties as statically linked libraries that will
        // be compiled into the wascc-provider binary.
        let cloned_host = host.clone();
        tokio::task::spawn_blocking(move || {
            info!("Loading HTTP capability");
            let http_provider = HttpServerProvider::new();
            let data = NativeCapability::from_instance(http_provider, None)
                .map_err(|e| anyhow::anyhow!("Failed to instantiate HTTP capability: {}", e))?;

            cloned_host
                .lock()
                .unwrap()
                .add_native_capability(data)
                .map_err(|e| anyhow::anyhow!("Failed to add HTTP capability: {}", e))?;

            info!("Loading log capability");
            let logging_provider = LoggingProvider::new();
            let logging_capability = NativeCapability::from_instance(logging_provider, None)
                .map_err(|e| anyhow::anyhow!("Failed to instantiate log capability: {}", e))?;
            cloned_host
                .lock()
                .unwrap()
                .add_native_capability(logging_capability)
                .map_err(|e| anyhow::anyhow!("Failed to add log capability: {}", e))
        })
        .await??;
        Ok(Self {
            shared: SharedPodState {
                client,
                handles: Default::default(),
                store,
                volume_path,
                log_path,
                host,
                port_map,
            },
        })
    }
}

struct ModuleRunContext {
    modules: HashMap<String, Vec<u8>>,
    volumes: HashMap<String, Ref>,
}

/// State that is shared between pod state handlers.
pub struct PodState {
    key: PodKey,
    run_context: ModuleRunContext,
    errors: usize,
    image_pull_backoff_strategy: ExponentialBackoffStrategy,
    crash_loop_backoff_strategy: ExponentialBackoffStrategy,
    shared: SharedPodState,
}

// No cleanup state needed, we clean up when dropping PodState.
#[async_trait]
impl kubelet::state::AsyncDrop for PodState {
    async fn async_drop(self) {
        {
            let mut lock = self.shared.port_map.lock().await;
            let ports_to_remove: Vec<u16> = lock
                .iter()
                .filter_map(|(k, v)| if v == &self.key { Some(*k) } else { None })
                .collect();
            debug!(
                "Pod {} in namespace {} releasing ports {:?}.",
                &self.key.name(),
                &self.key.namespace(),
                &ports_to_remove
            );
            for port in ports_to_remove {
                lock.remove(&port);
            }
        }
        {
            let mut handles = self.shared.handles.write().await;
            handles.remove(&self.key);
        }
    }
}

#[async_trait]
impl Provider for WasccProvider {
    type InitialState = Registered;
    type TerminatedState = Terminated;
    type PodState = PodState;

    const ARCH: &'static str = TARGET_WASM32_WASCC;

    async fn node(&self, builder: &mut Builder) -> anyhow::Result<()> {
        builder.set_architecture("wasm-wasi");
        builder.add_taint("NoSchedule", "kubernetes.io/arch", Self::ARCH);
        builder.add_taint("NoExecute", "kubernetes.io/arch", Self::ARCH);
        Ok(())
    }

    async fn initialize_pod_state(&self, pod: &Pod, pod_changed: Arc<Notify>) -> anyhow::Result<Self::PodState> {
        let run_context = ModuleRunContext {
            modules: Default::default(),
            volumes: Default::default(),
        };
        let key = PodKey::from(pod);
        Ok(PodState {
            key,
            run_context,
            errors: 0,
            image_pull_backoff_strategy: ExponentialBackoffStrategy::default(),
            crash_loop_backoff_strategy: ExponentialBackoffStrategy::default(),
            shared: self.shared.clone(),
        })
    }

    async fn logs(
        &self,
        namespace: String,
        pod_name: String,
        container_name: String,
        sender: kubelet::log::Sender,
    ) -> anyhow::Result<()> {
        let mut handles = self.shared.handles.write().await;
        let handle = handles
            .get_mut(&PodKey::new(&namespace, &pod_name))
            .ok_or_else(|| ProviderError::PodNotFound {
                pod_name: pod_name.clone(),
            })?;
        handle.output(&container_name, sender).await
    }
}

struct VolumeBinding {
    name: String,
    host_path: PathBuf,
}

/// Capability describes a waSCC capability.
///
/// Capabilities are made available to actors through a two-part processthread:
/// - They must be registered
/// - For each actor, the capability must be configured
struct Capability {
    name: &'static str,
    binding: Option<String>,
    env: EnvVars,
}

/// Holds our tempfile handle.
struct LogHandleFactory {
    temp: NamedTempFile,
}

impl kubelet::log::HandleFactory<tokio::fs::File> for LogHandleFactory {
    /// Creates `tokio::fs::File` on demand for log reading.
    fn new_handle(&self) -> tokio::fs::File {
        tokio::fs::File::from_std(self.temp.reopen().unwrap())
    }
}

/// Run the given WASM data as a waSCC actor with the given public key.
///
/// The provided capabilities will be configured for this actor, but the capabilities
/// must first be loaded into the host by some other process, such as register_native_capabilities().
fn wascc_run(
    host: Arc<Mutex<Host>>,
    data: Vec<u8>,
    env: EnvVars,
    volumes: Vec<VolumeBinding>,
    log_path: &Path,
    port_assigned: u16,
) -> anyhow::Result<ContainerHandle<ActorHandle, LogHandleFactory>> {
    let mut capabilities: Vec<Capability> = Vec::new();
    info!("sending actor to wascc host");
    let log_output = NamedTempFile::new_in(&log_path)?;

    let load =
        Actor::from_slice(&data).map_err(|e| anyhow::anyhow!("Error loading WASM: {}", e))?;
    let pk = load.public_key();

    let actor_caps = load.capabilities();

    if actor_caps.contains(&LOG_CAPABILITY.to_owned()) {
        let mut logenv = env.clone();
        logenv.insert(
            LOG_PATH_KEY.to_string(),
            log_output.path().to_str().unwrap().to_owned(),
        );
        capabilities.push(Capability {
            name: LOG_CAPABILITY,
            binding: None,
            env: logenv,
        });
    }

    if actor_caps.contains(&HTTP_CAPABILITY.to_owned()) {
        let mut httpenv = env.clone();
        httpenv.insert("PORT".to_string(), port_assigned.to_string());
        capabilities.push(Capability {
            name: HTTP_CAPABILITY,
            binding: None,
            env: httpenv,
        });
    }

    if actor_caps.contains(&FS_CAPABILITY.to_owned()) {
        for vol in &volumes {
            info!(
                "Loading File System capability for volume name: '{}' host_path: '{}'",
                vol.name,
                vol.host_path.display()
            );
            let mut fsenv = env.clone();
            fsenv.insert(
                FS_CONFIG_ROOTDIR.to_owned(),
                vol.host_path.as_path().to_str().unwrap().to_owned(),
            );
            let fs_provider = FileSystemProvider::new();
            let fs_capability =
                NativeCapability::from_instance(fs_provider, Some(vol.name.clone())).map_err(
                    |e| anyhow::anyhow!("Failed to instantiate File System capability: {}", e),
                )?;
            host.lock()
                .unwrap()
                .add_native_capability(fs_capability)
                .map_err(|e| anyhow::anyhow!("Failed to add File System capability: {}", e))?;
            capabilities.push(Capability {
                name: FS_CAPABILITY,
                binding: Some(vol.name.clone()),
                env: fsenv,
            });
        }
    }

    host.lock()
        .unwrap()
        .add_actor(load)
        .map_err(|e| anyhow::anyhow!("Error adding actor: {}", e))?;
    capabilities.iter().try_for_each(|cap| {
        info!("configuring capability {}", cap.name);
        host.lock()
            .unwrap()
            .set_binding(&pk, cap.name, cap.binding.clone(), cap.env.clone())
            .map_err(|e| anyhow::anyhow!("Error configuring capabilities for module: {}", e))
    })?;

    let log_handle_factory = LogHandleFactory { temp: log_output };

    info!("wascc actor executing");
    Ok(ContainerHandle::new(
        ActorHandle {
            host,
            key: pk,
            volumes,
            capabilities: actor_caps,
        },
        log_handle_factory,
    ))
}
