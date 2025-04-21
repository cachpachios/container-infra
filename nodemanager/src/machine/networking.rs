use anyhow::{Ok, Result};

pub fn cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    log::debug!("Running command: {} {}", cmd, args.join(" "));

    let out = command.spawn()?.wait_with_output()?;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "{} {} failed with status: {}",
            cmd,
            args.join(" "),
            out.status
        ));
    }
    Ok(())
}

pub struct TunTap {
    name: String,
}

impl TunTap {
    pub fn new(name: &str) -> Result<Self> {
        cmd("ip", &["tuntap", "add", name, "mode", "tap"])?;
        let tap = Self {
            name: name.to_string(),
        };
        Ok(tap)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn add_address(&self, cidr_addrs: &str) -> Result<()> {
        cmd("ip", &["addr", "add", cidr_addrs, "dev", &self.name])
    }

    pub fn up(&self) -> Result<()> {
        cmd("ip", &["link", "set", &self.name, "up"])
    }
}

impl Drop for TunTap {
    fn drop(&mut self) {
        let _ = cmd("ip", &["link", "del", &self.name]);
    }
}
