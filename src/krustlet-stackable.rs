use kube::config::Config as KubeConfig;
use kube::config::KubeConfigOptions;
use kubelet::config::Config;
use kubelet::store::composite::ComposableStore;
use kubelet::store::oci::FileStore;
use kubelet::Kubelet;
use pnet::datalink;
use pnet::ipnetwork::IpNetwork::V4;
use stackable_provider::StackableProvider;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main(threaded_scheduler)]
async fn main() -> anyhow::Result<()> {
    // The provider is responsible for all the "back end" logic. If you are creating
    // a new Kubelet, all you need to implement is a provider.

    let mut config = Config::new_from_file_and_flags(env!("CARGO_PKG_VERSION"), None);

    // Initialize the logger
    env_logger::init();

    //let kubeconfig = kubelet::bootstrap(&config, &config.bootstrap_file, notify_bootstrap).await?;
    let kubeconfig = KubeConfig::from_kubeconfig(&KubeConfigOptions::default())
        .await
        .expect("Failed to create Kubernetes Client!");

    let parcel_directory = PathBuf::from("/home/sliebau/IdeaProjects/krustlet/work/parcels");
    let config_directory = PathBuf::from("/home/sliebau/IdeaProjects/krustlet/work/config");
    let provider = StackableProvider::new(
        kube::Client::new(kubeconfig.clone()),
        parcel_directory,
        config_directory,
    )
    .await
    .expect("Error initializing provider.");

    let kubelet = Kubelet::new(provider, kubeconfig, config).await?;
    kubelet.start().await
}

fn get_default_ipaddress() -> Option<IpAddr> {
    let all_interfaces = datalink::interfaces();

    let default_interface = all_interfaces
        .iter()
        .filter(|e| e.is_up() && !e.is_loopback() && e.ips.len() > 0)
        .next();

    match default_interface {
        Some(interface) => {
            println!("Found default interface with [{:?}].", interface.ips);
            if let V4(test) = interface.ips[0] {
                if let ip_v4_address = test.ip() {
                    println!("found: {:?}", ip_v4_address);
                } else {
                    return None;
                }
            }
        }
        None => println!("Error while finding the default interface."),
    };
    return None;
}

fn notify_bootstrap(message: String) {
    println!("BOOTSTRAP: {}", message);
}
