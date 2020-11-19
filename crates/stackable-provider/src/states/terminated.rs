use kubelet::state::prelude::*;

use crate::PodState;
use crate::states::install_package::Installing;

#[derive(Default, Debug)]
/// The Pod failed to run.
// If we manually implement, we can allow for arguments.
pub struct Terminated {
    pub message: String,
}

#[async_trait::async_trait]
impl State<PodState> for Terminated {
    async fn next(self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {
        println!("terminated");
        Transition::Complete(Ok(()))
    }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Succeeded, &self.message)
    }
}
