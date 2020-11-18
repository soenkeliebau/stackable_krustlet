use kubelet::state::prelude::*;

use crate::PodState;
use crate::states::install_package::Installing;
use crate::states::download_package::Downloading;
use kubelet::backoff::BackoffStrategy;
use crate::repository::package::Package;
use log::{debug, info, error};

#[derive(Debug, TransitionTo)]
#[transition_to(Downloading)]
/// The Pod failed to run.
// If we manually implement, we can allow for arguments.
pub struct DownloadingBackoff {
    pub package: Package,
}

#[async_trait::async_trait]
impl State<PodState> for DownloadingBackoff {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        info!("Backing of before retrying download of package {}", self.package);
        pod_state.package_download_backoff_strategy.wait().await;
        Transition::next(self, Downloading)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, &"status:running")
    }
}
