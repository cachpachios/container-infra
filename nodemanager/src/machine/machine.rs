use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::machine::{firecracker, log::LogHandler};

use super::{
    firecracker::JailedCracker,
    networking::{self, TunTap},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub struct Machine {
    vm: JailedCracker,
    join_handle: tokio::task::JoinHandle<()>,
    nic: TunTap,
    log: Arc<Mutex<LogHandler>>,
}

#[derive(Deserialize)]
pub struct FirecrackerConfig {
    pub rootfs: PathBuf,
    pub kernel_image: PathBuf,
    pub jailer_binary: PathBuf,
    pub firecracker_binary: PathBuf,
}

pub struct MachineConfig {
    pub container_reference: String,
    pub vcpu_count: u8,
    pub mem_size_mb: u32,
}

impl Machine {
    pub async fn new(fc_config: &FirecrackerConfig, config: MachineConfig) -> Result<Self> {
        #[derive(Serialize)]
        struct Container {
            image: String,
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
        vm.set_boot(
                &fc_config.kernel_image,
                "console=ttyS0 quiet loglevel=1 reboot=k panic=-1 pci=off ip=172.16.0.2::172.16.0.1:255.255.255.252::eth0:off",
            )
            .await?;
        vm.set_rootfs(&fc_config.rootfs).await?;
        vm.create_drive(8, "drive0").await?;

        let tap = TunTap::new("tap0")?;
        tap.add_address("172.16.0.1/30")?;
        tap.up()?;
        vm.set_eth_tap(&tap).await?;

        let ours = "eno1";

        networking::cmd(
            "iptables-nft",
            &[
                "-t",
                "nat",
                "-A",
                "POSTROUTING",
                "-o",
                ours,
                "-s",
                "172.16.0.2",
                "-j",
                "MASQUERADE",
            ],
        )?;
        networking::cmd(
            "iptables-nft",
            &[
                "-A",
                "FORWARD",
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ],
        )?;

        networking::cmd(
            "iptables-nft",
            &[
                "-A",
                "FORWARD",
                "-i",
                tap.name(),
                "-o",
                ours,
                "-j",
                "ACCEPT",
            ],
        )?;

        let (log, jh) = LogHandler::spawn(out).await;

        let mut machine = Self {
            vm,
            nic: tap,
            join_handle: jh,
            log,
        };
        machine.vm.start_vm().await?;

        Ok(machine)
    }

    pub fn uuid(&self) -> &str {
        self.vm.uuid()
    }

    pub async fn shutdown(mut self) {
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
    }
}

impl Clone for Machine {
    fn clone(&self) -> Self {
        panic!("No!!!!")
    }
}
