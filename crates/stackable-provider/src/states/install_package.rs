use kubelet::state::{State, Transition};
use kubelet::pod::Pod;
use kubelet::state::prelude::*;
use crate::PodState;
use crate::states::running::Running;
use crate::states::failed::Failed;
use crate::states::create_config::CreatingConfig;
use crate::states::setup_failed::SetupFailed;
use log::{debug, info};
use kube::api::Meta;
use k8s_openapi::api::core::v1::PodSpec;
use crate::repository::package::Package;
use std::path::{Path, PathBuf};
use crate::error::StackableError;
use std::fs::File;
use flate2::read::GzDecoder;
use tar::Archive;

#[derive(Debug, TransitionTo)]
#[transition_to(CreatingConfig, SetupFailed)]
pub struct Installing {
    pub download_directory: PathBuf,
    pub parcel_directory: PathBuf,
    pub package: Package,
}

impl Installing {
    fn package_installed<T: Into<Package>>(&self, package: T) -> bool {
        let package = package.into();

        let package_file_name = self.parcel_directory.join(package.get_directory_name());
        debug!("Checking if package {:?} has already been installed to {:?}", package, package_file_name);
        Path::new(&package_file_name).exists()
    }

    fn get_target_directory(&self, package: Package) -> PathBuf {
        self.parcel_directory.join(package.get_directory_name())
    }

    fn install_package<T: Into<Package>>(&self, package: T) -> Result<(), StackableError> {
        let package: Package = package.into();
        // To be on the safe side, check if the package is actually there

        let archive_path = self.download_directory.join(package.get_file_name());
        let tar_gz = File::open(&archive_path)?;
        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);

        let target_directory = self.get_target_directory(package.clone());

        println!("Installing package: {:?} from {:?} into {:?}", package, archive_path, target_directory);
        archive.unpack(self.parcel_directory.join(package.get_directory_name()))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl State<PodState> for Installing {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        let package = self.package.clone();
        if self.package_installed(package.clone()) {
            info!("Package {} has already been installed", package);
            return Transition::next(self, CreatingConfig{ target_directory: None });
        } else {
            info!("Installing package {}", package);
            self.install_package(package.clone());
        }


        debug!("installing package");
        Transition::next(self, CreatingConfig{ target_directory: None })
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, &"status:initializing")
    }
}