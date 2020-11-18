use kubelet::provider::Provider;
use kubelet::log::Sender;
use kubelet::pod::{Pod, PodKey};

use crate::states::failed::Failed;
use kubelet::backoff::ExponentialBackoffStrategy;
use kubelet::node::Builder;
use crate::states::terminated::Terminated;
use crate::states::download_package::Downloading;
use kube::{Client, Api};
use crate::error::StackableError;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use crate::error::StackableError::CrdMissing;
use log::{debug, info, error};
use std::path::PathBuf;

pub struct StackableProvider {
    client: Client,
    parcel_directory: PathBuf,

}

pub const CRDS: &'static [&'static str] = &["repositories.stable.stackable.de"];


mod states;
mod repository;
mod error;

pub struct PodState {
    client: Client,
    parcel_directory: PathBuf,
    package_download_backoff_strategy: ExponentialBackoffStrategy,
}

impl StackableProvider {


    pub async fn new(client: Client, parcel_directory: PathBuf) -> Result<Self, StackableError> {
        let provider = StackableProvider {
            client,
            parcel_directory
        };
        let missing_crds = provider.check_crds().await;
        if missing_crds.is_empty() {
            debug!("All required CRDS present!");
            return Ok(provider);
        } else {
            debug!("Missing required CDRS");
            return Err(CrdMissing {missing_crds});
        }
    }

    async fn check_crds(&self) -> Vec<String>{
        let mut missing_crds = vec![];
        let crds: Api<CustomResourceDefinition> = Api::all(self.client.clone());

        // Check all CRDS
        for crd in CRDS.into_iter() {
            debug!("Checking if CRD \"{}\" is registered", crd);
            match crds.get(crd).await {
                Err(e) => {
                    error!("Missing required CRD: \"{}\"", crd);
                    missing_crds.push(String::from(*crd))
                },
                _ => {
                    debug!("Found registered crd: {}", crd)
                },
            }
        }
        missing_crds
    }
}

// No cleanup state needed, we clean up when dropping PodState.
#[async_trait::async_trait]
impl kubelet::state::AsyncDrop for PodState {
    async fn async_drop(self) {}
}

#[async_trait::async_trait]
impl Provider for StackableProvider {
    type PodState = PodState;
    type InitialState = Downloading;
    type TerminatedState = Terminated;


    const ARCH: &'static str = "stackable-linux";

    async fn node(&self, builder: &mut Builder) -> anyhow::Result<()> {
        builder.set_architecture(Self::ARCH);
        builder.add_taint("NoSchedule", "kubernetes.io/arch", Self::ARCH);
        builder.add_taint("NoExecute", "kubernetes.io/arch", Self::ARCH);
        Ok(())
    }

    async fn initialize_pod_state(&self, pod: &Pod) -> anyhow::Result<Self::PodState> {
        let parcel_directory = self.parcel_directory.clone();
        Ok(PodState {
            client: self.client.clone(),
            parcel_directory,
            package_download_backoff_strategy: ExponentialBackoffStrategy::default()
        })
    }

    async fn logs(&self, namespace: String, pod: String, container: String, sender: Sender) -> anyhow::Result<()> {
        Ok(())
    }
}
