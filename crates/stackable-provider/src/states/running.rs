use kubelet::state::{State, Transition};
use kubelet::pod::Pod;
use kubelet::state::prelude::*;
use crate::PodState;
use crate::states::failed::Failed;
use crate::states::stopping::Stopping;

#[derive(Default, Debug, TransitionTo)]
#[transition_to(Stopping, Failed)]
pub struct Running;


#[async_trait::async_trait]
impl State<PodState> for Running {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        for i in 1..8 {
            tokio::time::delay_for(std::time::Duration::from_secs(2)).await;
            println!("running");
        }
        Transition::next(self, Stopping)
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Pending, &"status:running")
    }
}