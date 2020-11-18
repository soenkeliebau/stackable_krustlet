use kubelet::state::{State, Transition};
use kubelet::pod::Pod;
use kubelet::state::prelude::*;
use crate::PodState;
use crate::states::running::Running;
use crate::states::failed::Failed;
use crate::states::create_service::CreatingService;
use crate::states::setup_failed::SetupFailed;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(CreatingService, SetupFailed)]
pub struct CreatingConfig;

#[async_trait::async_trait]
impl State<PodState> for CreatingConfig {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        println!("creating config");
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