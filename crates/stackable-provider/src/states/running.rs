use kubelet::state::{State, Transition};
use kubelet::pod::Pod;
use kubelet::state::prelude::*;
use crate::PodState;
use crate::states::failed::Failed;
use crate::states::stopping::Stopping;
use crate::states::install_package::Installing;
use kubelet::container::ContainerKey;
use log::{debug, info, warn, error};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use std::process::Child;
use crate::error::StackableError;

#[derive(Debug, TransitionTo)]
#[transition_to(Stopping, Failed, Running, Installing)]
pub struct Running {
    pub process_handle: Option<Child>,
}

impl Running {
    fn take_handle(&mut self) -> Child {
        debug!("testing");
        let mut handle = std::mem::replace(&mut self.process_handle, None);
        handle.unwrap()
    }
}

#[async_trait::async_trait]
impl State<PodState> for Running {
    async fn next(mut self: Box<Self>, pod_state: &mut PodState, _pod: &Pod) -> Transition<PodState> {

        debug!("waiting");
//        &self.wait();
//        warn!("process ended!");
//        Transition::next(self, Failed{ message: "process ended".to_string() })
        let mut changed = Arc::clone(&pod_state.pod_changed);
        //let mut handle = &self.take_handle();
        let mut handle = std::mem::replace(&mut self.process_handle, None).unwrap();
        while let Ok(_) = timeout(Duration::from_millis(100), changed.notified()).await {
            debug!("drained a waiting notification");
        }
        debug!("done draining");

        loop {
            println!("running");
            tokio::select! {
                _ = changed.notified() => {
                    debug!("pod changed");
                    break;
                },
                _ = tokio::time::delay_for(std::time::Duration::from_secs(1))  => {
                    debug!("timer expired");
                }
            }
            match handle.try_wait() {
                Ok(None) => debug!("Still running"),
                _ => {
                    error!("died");
                    return Transition::next(self, Failed { message: "process died".to_string() })
                }

            }
        }
        Transition::next(self, Installing{
            download_directory: pod_state.download_directory.clone(),
            parcel_directory: pod_state.parcel_directory.clone(),
            package: pod_state.package.clone()
        })
   }

    async fn json_status(
        &self,
        _pod_state: &mut PodState,
        _pod: &Pod,
    ) -> anyhow::Result<serde_json::Value> {
        make_status(Phase::Running, &"status:running")
    }
}