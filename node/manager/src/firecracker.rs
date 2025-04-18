use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
};

use anyhow::{Context, Result};
use log::debug;
use uuid::Uuid;

use crate::networking::TunTap;

pub struct JailedCracker {
    uuid: String,
    root_path: PathBuf,
    proc: Child,
    uid: u32,
}

impl JailedCracker {
    pub fn spawn(
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

        Ok((
            Self {
                uuid,
                root_path,
                proc: cmd,
                uid,
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

    pub fn wait(&mut self) {
        let _ = self.proc.wait();
    }

    pub fn set_rootfs(&self, path: &Path) -> Result<()> {
        let dest = self.root_path.join("root.fs");
        //TODO: Support not copying by default...
        debug!("Copying rootfs from {:?} to {:?}", path, dest);
        std::fs::copy(path, &dest)?;
        debug!("Chowning rootfs to {}", self.uid);
        std::os::unix::fs::chown(&dest, Some(self.uid), Some(self.uid))?;

        debug!("Putting rootfs in firecracker");
        self.request(
            "PUT",
            "/drives/rootfs",
            "{
            \"drive_id\": \"rootfs\",
            \"path_on_host\": \"/root.fs\",
            \"is_root_device\": true,
            \"is_read_only\": true}",
        )
        .map(|_| ())
    }

    pub fn create_drive(&self, size_gb: u64, drive_id: &str) -> Result<()> {
        let fp = self.root_path.join(format!("{}.fs", drive_id));
        debug!("Creating drive {} with size {}GB", drive_id, size_gb);
        let f = fs::File::create(&fp)?;
        f.set_len(size_gb * 1024 * 1024 * 1024)?;
        debug!("Chowning drive to {}", self.uid);
        std::os::unix::fs::chown(&fp, Some(self.uid), Some(self.uid))?;

        debug!("Putting rootfs in firecracker");
        self.request(
            "PUT",
            format!("/drives/{}", drive_id).as_str(),
            format!(
                "{{
            \"drive_id\": \"{}\",
            \"path_on_host\": \"/{}.fs\",
            \"is_root_device\": false,
            \"is_read_only\": false}}",
                drive_id, drive_id
            )
            .as_str(),
        )
    }

    pub fn set_eth_tap(&self, tap: &TunTap) -> Result<()> {
        self.add_network_interface("eth0", &tap.name)?;
        self.config_mmds("eth0")?;
        Ok(())
    }

    pub fn add_network_interface(&self, guest_name: &str, host_dev_name: &str) -> Result<()> {
        self.request(
            "PUT",
            format!("/network-interfaces/{}", guest_name).as_str(),
            format!(
                "{{\"iface_id\": \"{}\", \"host_dev_name\": \"{}\"}}",
                guest_name, host_dev_name
            )
            .as_str(),
        )
    }

    pub fn config_mmds(&self, guest_name: &str) -> Result<()> {
        self.request(
            "PUT",
            "/mmds/config",
            format!(
                "{{\"network_interfaces\": [\"{}\"], \"version\": \"V2\"}}",
                guest_name
            )
            .as_str(),
        )
    }

    pub fn set_boot(&self, kernel_img: &Path, boot_args: &str) -> Result<()> {
        let dest = self.root_path.join("kernel.img");
        //TODO: Mount this?
        debug!("Copying kernel from {:?} to {:?}", kernel_img, dest);
        std::fs::copy(kernel_img, &dest)?;
        debug!("Chowning kernel to {}", self.uid);
        std::os::unix::fs::chown(&dest, Some(self.uid), Some(self.uid))?;

        debug!("Setting boot source to kernel.img in firecracker");
        self.request(
            "PUT",
            "/boot-source",
            format!(
                "{{\"kernel_image_path\": \"/kernel.img\", \"boot_args\": \"{}\"}}",
                boot_args
            )
            .as_str(),
        )
    }

    pub fn set_machine_config(&self, vcpu_count: u8, mem_size_mb: u32) -> Result<()> {
        self.request(
            "PUT",
            "/machine-config",
            format!(
                "{{\"vcpu_count\": {}, \"mem_size_mib\": {}}}",
                vcpu_count, mem_size_mb
            )
            .as_str(),
        )
    }

    pub fn start_vm(&self) -> Result<()> {
        self.request("PUT", "/actions", "{ \"action_type\": \"InstanceStart\" }")
    }

    fn request(&self, method: &str, route: &str, data: &str) -> Result<()> {
        //TODO: Dont use curl. Use something rust native. Whenever something reasonable to send HTTP over unix sockets is available.
        let mut cmd = Command::new("curl");
        cmd.arg("-X").arg(method);
        cmd.arg("--data").arg(data);
        cmd.arg("--unix-socket")
            .arg(self.root_path.join("run").join("firecracker.socket"));
        cmd.arg(format!("http://localhost{}", route));
        let output = cmd.output()?;
        if !output.status.success() {
            return Err(anyhow::Error::msg(format!(
                "Request failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }
}
