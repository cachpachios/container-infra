use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use crate::{machine::firecracker, networking::NetworkStack};

use super::{firecracker::JailedCracker, vsock::MachineCommunicator};
use anyhow::Result;
use log::trace;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use vmproto::guest::LogMessage;

pub struct Machine {
    vm: JailedCracker,
    comm: Option<(Arc<Mutex<MachineCommunicator>>, tokio::task::JoinHandle<()>)>,
    network: Mutex<NetworkStack>,
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
        struct Config {
            image: String,
            cmd_args: Option<Vec<String>>,
            env: Option<BTreeMap<String, String>>,
            vsock_port: u32,
        }

        #[derive(Serialize)]
        struct Metadata {
            container: Config,
        }

        #[derive(Serialize)]
        struct Latest {
            latest: Metadata,
        }

        let vsock_port = rand::random::<u32>() % (u32::MAX - 4) + 3;

        let metadata = Metadata {
            container: Config {
                image: config.container_reference,
                cmd_args: overrides.cmd_args,
                env: overrides.env,
                vsock_port,
            },
        };

        let metadata = serde_json::to_string(&Latest { latest: metadata })?;

        let mut vm = firecracker::JailedCracker::spawn(
            &fc_config.jailer_binary,
            &fc_config.firecracker_binary,
            0,
            Some(&metadata),
        )
        .await?;

        vm.set_machine_config(config.vcpu_count, config.mem_size_mb)
            .await?;
        let boot_args: String = format!(
            "8250.nr_uarts=0 quiet loglevel=1 reboot=k panic=-1 pci=off ip={}::{}:{}::eth0:off",
            network_stack.ipv4_addr(),
            network_stack.gateway(),
            network_stack.subnet_mask(),
        );
        trace!("Setting kernel boot args: {}", boot_args);
        vm.set_boot(&fc_config.kernel_image, &boot_args).await?;
        vm.set_rootfs(&fc_config.rootfs).await?;
        vm.create_drive(8, "drive0").await?;
        vm.set_eth_tap(network_stack.nic()).await?;

        let listener = vm.open_vsock_listener(vsock_port).await?;

        let mut machine = Self {
            vm,
            network: Mutex::new(network_stack),
            comm: None,
        };
        machine.vm.start_vm().await?;

        let connection = tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;

        let stream = match connection {
            Ok(Ok((stream, _))) => stream,
            Ok(Err(e)) => {
                log::error!("Failed to accept vsock connection: {}", e);
                return Err(e.into());
            }
            Err(_) => {
                log::error!("Timeout accepting vsock connection");
                return Err(anyhow::anyhow!("Timeout accepting vsock connection"));
            }
        };

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        machine.comm = Some(MachineCommunicator::spawn(stream, stop_tx).await);

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

        if let Some((_, join_handle)) = self.comm.take() {
            let _ = tokio::time::timeout(MAX_WAIT, join_handle)
                .await
                .map_err(|_| {
                    log::warn!("Timeout waiting for communication channel to close");
                });
        }

        let _ = self.vm.cleanup();
        self.network.into_inner()
    }

    pub async fn get_and_subscribe_to_logs(
        &self,
    ) -> Result<(
        Vec<Arc<LogMessage>>,
        tokio::sync::mpsc::Receiver<Arc<LogMessage>>,
    )> {
        let comm = self
            .comm
            .as_ref()
            .ok_or(anyhow::anyhow!("Communication never initialized"))?;
        let mut handler = comm.0.lock().await;
        Ok((handler.clone_buffer(), handler.subscribe_log()))
    }

    pub async fn get_logs(&self) -> Result<Vec<Arc<LogMessage>>> {
        let comm = self
            .comm
            .as_ref()
            .ok_or(anyhow::anyhow!("Communication never initialized"))?;
        let handler = comm.0.lock().await;
        Ok(handler.clone_buffer())
    }
}
