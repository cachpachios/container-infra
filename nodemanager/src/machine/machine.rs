use futures_lite::io::AsyncReadExt;
use std::{io::Write, path::PathBuf, time::Duration};
use tokio::time::sleep;

use crate::machine::firecracker;

use super::{
    firecracker::JailedCracker,
    networking::{self, TunTap},
};
use anyhow::Result;
use async_process::ChildStdout;
use serde::{Deserialize, Serialize};

pub struct Machine {
    vm: JailedCracker,
    join_handle: Option<tokio::task::JoinHandle<()>>,
    nic: TunTap,
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

        let (mut vm, mut out) = firecracker::JailedCracker::spawn(
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

        let jh = tokio::spawn(stdout_handler(out));

        vm.start_vm().await?;

        Ok(Self {
            vm,
            join_handle: Some(jh),
            nic: tap,
        })
    }

    pub fn uuid(&self) -> &str {
        self.vm.uuid()
    }

    pub async fn shutdown(&mut self) {
        let _ = self.vm.request_stop().await;

        const MAX_WAIT: Duration = Duration::from_secs(3);

        if let Some(jh) = self.join_handle.take() {
            let e = tokio::time::timeout(MAX_WAIT, jh).await;
            if let Err(_) = e {
                log::warn!(
                    "Timeout {}s waiting to shutdown VM {}, killing it",
                    MAX_WAIT.as_secs(),
                    self.vm.uuid()
                );
            }
        }
        self.cleanup();
    }

    fn cleanup(&mut self) {
        let _ = self.vm.cleanup();
        if let Some(jh) = self.join_handle.take() {
            let _ = jh.abort();
        }
    }
}

impl Clone for Machine {
    fn clone(&self) -> Self {
        panic!("No!!!!")
    }
}

async fn stdout_handler(mut out: ChildStdout) {
    let mut buf = [0; 1024];
    let mut our = std::io::stdout();
    loop {
        match out.read(&mut buf).await {
            Ok(0) => {
                log::debug!("Firecracker process exited");
                break;
            }
            Ok(n) => {
                our.write_all(&buf[..n]).expect("Unable to write to stderr");
            }
            Err(_) => {
                log::error!("Error reading from firecracker stdout?");
                break;
            }
        }
    }
}
