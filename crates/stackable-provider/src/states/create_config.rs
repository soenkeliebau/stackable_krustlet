use kubelet::state::{State, Transition};
use kubelet::pod::Pod;
use kubelet::state::prelude::*;
use crate::PodState;
use crate::states::running::Running;
use crate::states::failed::Failed;
use crate::states::create_service::CreatingService;
use crate::states::setup_failed::SetupFailed;
use log::{debug, info, warn, error};
use crate::fail_fatal;
use crate::error::StackableError::{PodValidationError, RuntimeError};
use std::collections::HashMap;
use k8s_openapi::api::core::v1::{VolumeMount, Volume, ConfigMap};
use crate::error::StackableError;
use std::path::PathBuf;
use kube::{Api, Client};
use kube::api::ListParams;
use std::fs;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(CreatingService, SetupFailed)]
pub struct CreatingConfig {
    pub target_directory: Option<PathBuf>,
}

impl CreatingConfig {
    async fn retrieve_config_map(&self, client: Client, name: String) -> Result<ConfigMap, StackableError> {
        let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), "default");

        Ok(config_maps.get(&name).await?)
    }

    fn apply_config_map(&self, map: ConfigMap, target_directory: PathBuf) -> Result<(), StackableError> {
        debug!("applying configmap {} to directory {:?}", map.metadata.name.unwrap_or(String::from("undefined")), target_directory);
        if !(&target_directory.is_dir()) {
            info!("creating config directory {:?}", target_directory);
            fs::create_dir_all(&target_directory)?;
        }
        if let Some(data) = map.data {
            for key in data.keys() {
                debug!("found key: {} in configmap", key);
                if let Some(content) = data.get(key) {
                    debug!("content of key: {}", content);
                    let target_file = target_directory.join(&key);
                    debug!("writing content of map entry {} to file {:?}", key, target_file);
                    let write_result = fs::write(target_directory.join(&key), content);
                    match write_result {
                        Ok(()) => debug!("write of file {:?} successful!", target_file),
                        Err(e) => error!("write of file {:?} failed: {}", target_file, e)
                    }
                } else {
                    info!("No content found for key {}", key);
                }
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl State<PodState> for CreatingConfig {
    async fn next(mut self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        let name = _pod.name();
        let client = pod_state.client.clone();
        let package = pod_state.package.clone();
        let config_directory = pod_state.config_directory.clone();
        self.target_directory = Some(config_directory.join(package.get_directory_name()));
        let target_directory = self.target_directory.clone().unwrap();

        debug!("Entering state \"creating config\" for service {}", name);
        let containers = _pod.containers();
        if containers.len().ne(&1) {
            let e = PodValidationError { msg: "Only pods containing exactly one container element are supported!".to_string() };
            fail_fatal!(e);
        }
        let container = containers[0].clone();

        if let Some(volumes) = _pod.volumes() {
            debug!("Found {} volumes in pod {}", volumes.len(), _pod.name());
            if let Some(mounts) = container.volume_mounts() {
                debug!("Found {} mounts in pod {}", mounts.len(), _pod.name());
                // Got mounts and volumes, we can now decide which ones we need to act upon
                for mount in mounts {
                    for volume in volumes {
                        if mount.name.eq(&volume.name) {
                            let target_dir = target_directory.join(&mount.mount_path.trim_start_matches('/'));
                            if let Some(config_map) = &volume.config_map {
                                if let Some(map_name) = &config_map.name {
                                    if let Ok(map) = self.retrieve_config_map(client.clone(), map_name.to_string()).await {
                                        debug!("found config map: {:?}", config_map);
                                        self.apply_config_map(map, target_dir);
                                    }
                                }
                            } else {
                                warn!("Skipping volume {} - it is not a config map", volume.name);
                            }
                        }
                    }
                }
            };
        }

        Transition::next(self, CreatingService)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, &"status:initializing")
    }
}