use kubelet::state::{State, Transition};
use kubelet::pod::Pod;
use kubelet::state::prelude::*;
use crate::PodState;
use crate::states::running::Running;
use crate::states::failed::Failed;
use crate::states::install_package::Installing;
use crate::states::setup_failed::SetupFailed;
use crate::error::StackableError::PodValidationError;
use crate::fail_fatal;
use kube::api::Meta;
use k8s_openapi::api::core::v1::PodSpec;
use crate::repository::package::Package;
use crate::error::StackableError;
use kubelet::container::Container;
use std::convert::TryFrom;
use log::{debug, info, error};
use crate::repository::find_repository;
use crate::states::download_package_backoff::DownloadingBackoff;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(Installing, DownloadingBackoff)]
pub struct Downloading;

impl Downloading {
    fn get_package(&self, pod: &Pod) -> Result<Package, StackableError> {
        let containers = pod.containers();
        if (containers.len().ne(&1)) {
            let e = PodValidationError { msg: String::from("Size of containers list in PodSpec has to be exactly 1") };
            return Err(e);
        } else {
            // List has exactly one value, try to parse this
            if let Ok(Some(reference)) = containers[0].image() {
                return Package::try_from(reference);
            } else {
                let e = PodValidationError { msg: String::from("Unable to get package reference from pod") };
                return Err(e);
            }
        }
    }
}

#[async_trait::async_trait]
impl State<PodState> for Downloading {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        let package = &self.get_package(_pod);
        match package {
            Ok(package) => {
                info!("Looking for package: {} in known repositories", &package);
                let repo = find_repository(pod_state.client.clone(), package, None).await;
                match repo {
                    Ok(Some(repo)) => {
                        // We found a repository providing the package, proceed with download
                        // The repository has already downloaded its metadata it this time, as that
                        // was used to check whether it provides the package
                        info!("Starting download of package {} from repository {}", &package, &repo);
                        //repo.download_package()
                    },
                    Ok(None) => {
                        // No repository was found that provides this package
                        let message = format!("Cannot find package {} in any repository, aborting ..", &package);
                        error!("{}", &message);
                        return Transition::next(self, DownloadingBackoff { package: package.clone() } );
                    },
                    Err(e) => {
                        // An error occurred when looking for a repository providing this package
                        let message = format!("Error occurred trying to find package {}: {}", &package, e);
                        error!("{}", &message);
                        return Transition::next(self, DownloadingBackoff { package: package.clone() } );

                    },
                }

            }
            Err(e) => {
                error!("Error parsing package: {}", e);
                //fail_fatal!(e);
            }
        }


        Transition::next(self, Installing)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, &"status:initializing")
    }
}

