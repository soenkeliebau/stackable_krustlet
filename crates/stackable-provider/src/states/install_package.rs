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

#[derive(Default, Debug, TransitionTo)]
#[transition_to(CreatingConfig, SetupFailed)]
pub struct Installing;

#[async_trait::async_trait]
impl State<PodState> for Installing {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {



        debug!("installing package");
        Transition::next(self, CreatingConfig)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, &"status:initializing")
    }
}