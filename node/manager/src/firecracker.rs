use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{anyhow, Context, Result};
use async_process::{Child, ChildStdout, Command};
use http_client_unix_domain_socket::{ClientUnix, Method};
use log::debug;
use serde::Serialize;
use uuid::Uuid;

use crate::networking::TunTap;

#[derive(Serialize)]
struct Drive {
    // cache_type
    drive_id: String,
    //io_engine
    is_read_only: bool,
    is_root_device: bool,
    //partuuid
    path_on_host: String,
    //rate_limiter
    //socket
}

#[derive(Serialize)]
struct NetworkInterface {
    //guest_mac
    host_dev_name: String,
    iface_id: String,
    //rx_rate_limiter
    //tx_rate_limiter
}

#[derive(Serialize)]
enum MmdsVersion {
    // V1,
    V2,
}

#[derive(Serialize)]
struct MmdsConfig {
    //ipv4_address
    network_interfaces: Vec<String>,
    version: MmdsVersion,
}

#[derive(Serialize)]
struct BootSource {
    kernel_image_path: String,
    boot_args: String,
}

#[derive(Serialize)]
struct MachineConfig {
    // cpu_template
    // huge_pages
    mem_size_mib: u64,
    // smt
    // track_dirty_pages
    vcpu_count: u8, // Max 32
}

#[derive(Serialize)]
enum InstanceAction {
    InstanceStart,
    // SendCtrlAltDel,
    // FlushMetrics,
}

#[derive(Serialize)]
struct InstanceActionInfo {
    action_type: InstanceAction,
}

pub struct JailedCracker {
    uuid: String,
    root_path: PathBuf,
    proc: Child,
    uid: u32,
    api_client: ClientUnix,
}

impl JailedCracker {
    pub async fn spawn(
        jailer_bin: &Path,
        firecracker_bin: &Path,
        uid_offset: u16,
        mmds_json: Option<&str>,
    ) -> Result<(Self, ChildStdout)> {
        let uuid: String = Uuid::new_v4().to_string();
        debug!("Starting jailed firecracker with id {}", uuid);

        let mut cmd = Command::new(jailer_bin);
        cmd.env_clear();
        cmd.arg("--id").arg(&uuid);
        cmd.arg("--exec-file").arg(firecracker_bin);
        let uid: u32 = 10000 + uid_offset as u32;
        cmd.arg("--uid").arg(uid.to_string());
        cmd.arg("--gid").arg(uid.to_string());

        cmd.arg("--");
        cmd.arg("--level").arg("error");

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        // cmd.stdin(Stdio::null());

        let fc_bin = firecracker_bin
            .file_name()
            .ok_or(anyhow::Error::msg("Unable to get firecracker binary name"))?;
        let root_path = Path::new("/srv/jailer/")
            .join(fc_bin)
            .join(&uuid)
            .join("root");

        //Mkdirs
        std::fs::create_dir_all(&root_path).context(format!(
            "Unable to create jailer root path {}",
            root_path.display()
        ))?;

        if let Some(mmds_json) = mmds_json {
            std::fs::write(&root_path.join("metadata.json"), mmds_json)?;
            cmd.arg("--metadata").arg("metadata.json");
        }

        let mut cmd = cmd.spawn()?;

        let stdout = cmd.stdout.take().ok_or(anyhow::Error::msg(
            "Unable to get stdout from jailer process",
        ))?;

        // Wait for jailer to start firecracker and create socket, max 1ms
        for _ in 0..20 {
            let socket_path = root_path.join("run").join("firecracker.socket");
            if socket_path.exists() {
                break;
            }
            debug!("Waiting for socket to be created... Sleeping 50us");
            tokio::time::sleep(std::time::Duration::from_micros(50)).await;
        }

        let api_client = ClientUnix::try_new(
            &root_path
                .join("run")
                .join("firecracker.socket")
                .to_str()
                .ok_or(anyhow!("Unable to get path to API socket."))?,
        )
        .await?;

        Ok((
            Self {
                uuid,
                root_path,
                proc: cmd,
                uid,
                api_client,
            },
            (stdout),
        ))
    }

    pub fn cleanup(mut self) -> Result<()> {
        let _ = self.proc.kill();
        std::fs::remove_dir_all(&self.root_path.parent().unwrap())?;
        Ok(())
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub async fn set_rootfs(&mut self, path: &Path) -> Result<()> {
        let dest = self.root_path.join("root.fs");
        debug!("Copying rootfs from {:?} to {:?}", path, dest);
        std::fs::copy(path, &dest)?;
        std::os::unix::fs::chown(&dest, Some(self.uid), Some(self.uid))?;

        debug!("Putting rootfs in firecracker");

        let drive = Drive {
            drive_id: "rootfs".into(),
            is_read_only: true,
            is_root_device: true,
            path_on_host: "/root.fs".into(),
        };

        self.request_with_json("/drives/rootfs", Method::PUT, &drive)
            .await
    }

    pub async fn create_drive(&mut self, size_gb: u64, drive_id: &str) -> Result<()> {
        let fp = self.root_path.join(format!("{}.fs", drive_id));
        debug!("Creating drive {} with size {}GB", drive_id, size_gb);
        let f = std::fs::File::create(&fp)?;
        f.set_len(size_gb * 1024 * 1024 * 1024)?;
        debug!("Chowning drive to {}", self.uid);
        std::os::unix::fs::chown(&fp, Some(self.uid), Some(self.uid))?;

        debug!("Putting rootfs in firecracker");
        let drive_config = Drive {
            drive_id: drive_id.into(),
            is_read_only: false,
            is_root_device: false,
            path_on_host: format!("/{}.fs", drive_id),
        };

        self.request_with_json(
            format!("/drives/{}", drive_id).as_str(),
            Method::PUT,
            &drive_config,
        )
        .await
    }

    pub async fn set_eth_tap(&mut self, tap: &TunTap) -> Result<()> {
        self.add_network_interface("eth0", &tap.name).await?;
        self.config_mmds("eth0").await?;
        Ok(())
    }

    pub async fn add_network_interface(
        &mut self,
        guest_name: &str,
        host_dev_name: &str,
    ) -> Result<()> {
        let interface = NetworkInterface {
            host_dev_name: host_dev_name.into(),
            iface_id: guest_name.into(),
        };
        self.request_with_json(
            format!("/network-interfaces/{}", guest_name).as_str(),
            Method::PUT,
            &interface,
        )
        .await
    }

    pub async fn config_mmds(&mut self, guest_name: &str) -> Result<()> {
        let config = MmdsConfig {
            network_interfaces: vec![guest_name.into()],
            version: MmdsVersion::V2,
        };
        self.request_with_json("/mmds/config", Method::PUT, &config)
            .await
    }

    pub async fn set_boot(&mut self, kernel_img: &Path, boot_args: &str) -> Result<()> {
        let dest = self.root_path.join("kernel.img");
        //TODO: Mount this?
        debug!("Copying kernel from {:?} to {:?}", kernel_img, dest);
        std::fs::copy(kernel_img, &dest)?;
        debug!("Chowning kernel to {}", self.uid);
        std::os::unix::fs::chown(&dest, Some(self.uid), Some(self.uid))?;

        debug!("Setting boot source to kernel.img in firecracker");
        let boot_source = BootSource {
            kernel_image_path: "/kernel.img".into(),
            boot_args: boot_args.into(),
        };
        self.request_with_json("/boot-source", Method::PUT, &boot_source)
            .await
    }

    pub async fn set_machine_config(&mut self, vcpu_count: u8, mem_size_mb: u32) -> Result<()> {
        let config = MachineConfig {
            vcpu_count,
            mem_size_mib: mem_size_mb as u64,
        };
        self.request_with_json("/machine-config", Method::PUT, &config)
            .await
    }

    pub async fn start_vm(&mut self) -> Result<()> {
        self.request_with_json(
            "/actions",
            Method::PUT,
            &InstanceActionInfo {
                action_type: InstanceAction::InstanceStart,
            },
        )
        .await
    }

    async fn request_with_json<T: Serialize>(
        &mut self,
        route: &str,
        method: Method,
        data: &T,
    ) -> Result<()> {
        let json_data = serde_json::to_string(data)
            .with_context(|| format!("Unable to serialize FC request to {}", route))?;

        match self
            .api_client
            .send_request(
                route,
                method,
                &vec![("Host", "localhost")],
                Some(http_client_unix_domain_socket::Body::from(json_data)),
            )
            .await
        {
            Err(e) => return Err(anyhow!("Firecracker API request failed: {}", e)),
            Ok((status_code, _)) => {
                if !status_code.is_success() {
                    return Err(anyhow!(
                        "Firecracker API request failed with status code: {}",
                        status_code
                    ));
                }
                Ok(())
            }
        }
    }
}
