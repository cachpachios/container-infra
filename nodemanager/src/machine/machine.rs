use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use crate::{
    machine::{firecracker, log::LogHandler},
    networking::NetworkStack,
};

use super::firecracker::JailedCracker;
use anyhow::Result;
use log::trace;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub struct Machine {
    vm: JailedCracker,
    join_handle: tokio::task::JoinHandle<()>,
    network: Mutex<NetworkStack>,
    log: Arc<Mutex<LogHandler>>,
}

pub struct MachineConfig {
    pub container_reference: String,
    pub vcpu_count: u8,
    pub mem_size_mb: u32,
}

pub struct ContainerOverrides {
    pub cmd_args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
pub struct FirecrackerConfig {
    pub rootfs: PathBuf,
    pub kernel_image: PathBuf,
    pub jailer_binary: PathBuf,
    pub firecracker_binary: PathBuf,
}

impl Machine {
    pub async fn new(
        fc_config: &FirecrackerConfig,
        config: MachineConfig,
        network_stack: NetworkStack,
        overrides: ContainerOverrides,
    ) -> Result<(Self, tokio::sync::oneshot::Receiver<()>)> {
        #[derive(Serialize)]
        struct Container {
            image: String,
            cmd_args: Option<Vec<String>>,
            env: Option<BTreeMap<String, String>>,
        }

        #[derive(Serialize)]
        struct Metadata {
            container: Container,
        }

        #[derive(Serialize)]
        struct Latest {
            latest: Metadata,
        }

        let metadata = Metadata {
            container: Container {
                image: config.container_reference,
                cmd_args: overrides.cmd_args,
                env: overrides.env,
            },
        };

        let metadata = serde_json::to_string(&Latest { latest: metadata })?;

        let (mut vm, out) = firecracker::JailedCracker::spawn(
            &fc_config.jailer_binary,
            &fc_config.firecracker_binary,
            0,
            Some(&metadata),
        )
        .await?;

        vm.set_machine_config(config.vcpu_count, config.mem_size_mb)
            .await?;
        let boot_args: String = format!(
            "console=ttyS0 quiet loglevel=1 reboot=k panic=-1 pci=off ip={}::{}:{}::eth0:off",
            network_stack.ipv4_addr(),
            network_stack.gateway(),
            network_stack.subnet_mask(),
        );
        trace!("Setting kernel boot args: {}", boot_args);
        vm.set_boot(&fc_config.kernel_image, &boot_args).await?;
        vm.set_rootfs(&fc_config.rootfs).await?;
        vm.create_drive(8, "drive0").await?;
        vm.set_eth_tap(network_stack.nic()).await?;

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

        let (log, jh) = LogHandler::spawn(out, stop_tx).await;

        let mut machine = Self {
            vm,
            network: Mutex::new(network_stack),
            join_handle: jh,
            log,
        };
        machine.vm.start_vm().await?;

        Ok((machine, stop_rx))
    }

    pub fn uuid(&self) -> &str {
        self.vm.uuid()
    }

    pub fn network(&self) -> &Mutex<NetworkStack> {
        &self.network
    }

    pub async fn shutdown(mut self) -> NetworkStack {
        let _ = self.vm.request_stop().await;

        const MAX_WAIT: Duration = Duration::from_secs(3);

        let e = tokio::time::timeout(MAX_WAIT, self.join_handle).await;
        if let Err(_) = e {
            log::warn!(
                "Timeout {}s waiting to shutdown VM {}, killing it",
                MAX_WAIT.as_secs(),
                self.vm.uuid()
            );
        }
        let _ = self.vm.cleanup();
        self.network.into_inner()
    }

    pub async fn get_and_subscribe_to_logs(
        &self,
    ) -> (Vec<String>, tokio::sync::mpsc::Receiver<Arc<str>>) {
        let mut handler = self.log.lock().await;
        (handler.clone_buffer(), handler.subscribe())
    }

    pub async fn get_logs(&self) -> Vec<String> {
        let handler = self.log.lock().await;
        handler.clone_buffer()
    }
}

impl Clone for Machine {
    fn clone(&self) -> Self {
        panic!("No!!!!")
    }
}
