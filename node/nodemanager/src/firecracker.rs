use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
};

use anyhow::Result;
use uuid::Uuid;

pub struct JailedCracker {
    root_path: PathBuf,
    proc: Child,
    uid: u32,
}

impl JailedCracker {
    pub fn new(jailer_bin: &Path, firecracker_bin: &Path, uid_offset: u16) -> Self {
        let uuid = Uuid::new_v4().to_string();

        let mut cmd = Command::new(jailer_bin);
        cmd.env_clear();
        cmd.arg("--id").arg(&uuid);
        cmd.arg("--exec-file").arg(firecracker_bin);
        let uid: u32 = 10000 + uid_offset as u32;
        cmd.arg("--uid").arg(uid.to_string());
        cmd.arg("--gid").arg(uid.to_string());

        let root_path = Path::new("/srv/jailer/")
            .join(
                firecracker_bin
                    .file_name()
                    .expect("Firecracker bin has no file stem"),
            )
            .join(&uuid)
            .join("root");

        Self {
            root_path,
            proc: cmd.spawn().expect("Unable to start jailer"),
            uid,
        }
    }

    pub fn cleanup(mut self) {
        let _ = self.proc.kill();
        std::fs::remove_dir_all(&self.root_path.parent().unwrap())
            .expect("Unable to cleanup rootfs");
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn set_rootfs(&self, path: &Path) -> Result<()> {
        let dest = self.root_path.join("root.fs");
        //TODO: Support not copying by default...
        std::fs::copy(path, &dest)?;
        std::os::unix::fs::chown(&dest, Some(self.uid), Some(self.uid))?;

        self.request(
            "PUT",
            "/drives/rootfs",
            "{
            \"drive_id\": \"rootfs\",
            \"path_on_host\": \"/root.fs\",
            \"is_root_device\": true,
            \"is_read_only\": false}",
        )
        .map(|_| ())
    }

    pub fn set_boot(&self, kernel_img: &Path) -> Result<()> {
        let dest = self.root_path.join("kernel.img");

        //TODO: Mount this?
        std::fs::copy(kernel_img, &dest)?;
        std::os::unix::fs::chown(&dest, Some(self.uid), Some(self.uid))?;

        self.request(
            "PUT",
            "/boot-source",
            "{\"kernel_image_path\": \"/kernel.img\", \"boot_args\": \"console=ttyS0 reboot=k panic=1 pci=off\"}"
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
