use std::net::Ipv4Addr;

use anyhow::{Ok, Result};

pub fn cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    log::trace!("Running command: {} {}", cmd, args.join(" "));

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

pub struct NetworkStack {
    ipv4_addr: Ipv4Addr,
    gateway: Ipv4Addr,
    nic: TunTap,
    chain_rules: Vec<Vec<String>>,
}

impl NetworkStack {
    fn new(slot: NetworkStackSlot) -> Result<Self> {
        let tap = TunTap::new(&slot.tap_dev_name)?;
        tap.add_address(&format!("{}/30", slot.gateway))?;
        tap.up()?;

        Ok(Self {
            ipv4_addr: slot.ipv4_addr,
            gateway: slot.gateway,
            nic: tap,
            chain_rules: Vec::new(),
        })
    }

    fn add_ip_rule(&mut self, args: &[&str]) -> Result<()> {
        cmd("iptables-nft", args)?;
        self.chain_rules
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }

    pub fn setup_public_nat(&mut self, outbound_if_name: &str) -> Result<()> {
        let addr = self.ipv4_addr.to_string();
        let nic_name = self.nic.name().to_owned(); // Stupid borrow checker
        self.add_ip_rule(&[
            "-t",
            "nat",
            "-A",
            "POSTROUTING",
            "-o",
            outbound_if_name,
            "-s",
            &addr,
            "-j",
            "MASQUERADE",
        ])?;
        self.add_ip_rule(&[
            "-A",
            "FORWARD",
            "-m",
            "conntrack",
            "--ctstate",
            "RELATED,ESTABLISHED",
            "-j",
            "ACCEPT",
        ])?;

        self.add_ip_rule(&[
            "-A",
            "FORWARD",
            "-i",
            &nic_name,
            "-o",
            outbound_if_name,
            "-j",
            "ACCEPT",
        ])?;
        Ok(())
    }

    pub fn ipv4_addr(&self) -> &Ipv4Addr {
        &self.ipv4_addr
    }

    pub fn gateway(&self) -> &Ipv4Addr {
        &self.gateway
    }

    pub fn subnet_mask(&self) -> &str {
        "255.255.255.252"
    }

    pub fn nic(&self) -> &TunTap {
        &self.nic
    }

    fn reclaim(self) -> NetworkStackSlot {
        NetworkStackSlot {
            ipv4_addr: self.ipv4_addr,
            gateway: self.gateway,
            tap_dev_name: self.nic.name().to_string(),
        }
    }
}

impl Drop for NetworkStack {
    fn drop(&mut self) {
        for rule in &self.chain_rules {
            let args = ["--delete"]
                .into_iter()
                .chain(rule.iter().map(|s| s.as_str()))
                .collect::<Vec<_>>();
            let _ = cmd("iptables-nft", &args);
        }
    }
}

struct NetworkStackSlot {
    ipv4_addr: Ipv4Addr,
    gateway: Ipv4Addr,
    tap_dev_name: String,
}

pub struct NetworkManager {
    recovered_slots: Vec<NetworkStackSlot>,
    next_id: u16,
}

impl NetworkManager {
    pub fn new() -> Self {
        Self {
            recovered_slots: Vec::new(),
            next_id: 0,
        }
    }

    fn next_slot(&mut self) -> NetworkStackSlot {
        if let Some(slot) = self.recovered_slots.pop() {
            return slot;
        }

        let id = self.next_id;
        self.next_id += 1;

        let tap_dev_name = format!("tap{}", id);

        let ip_id = id * 4 + 1;

        let first_half = (ip_id >> 8) as u8;
        let second_half = (ip_id & 0xFF) as u8;
        let gateway = Ipv4Addr::new(172, 16, first_half, second_half);
        let ipv4_addr = Ipv4Addr::new(172, 16, first_half, second_half + 1);
        NetworkStackSlot {
            ipv4_addr,
            gateway,
            tap_dev_name,
        }
    }

    pub fn provision_stack(&mut self) -> Result<NetworkStack> {
        let slot = self.next_slot();
        let stack = NetworkStack::new(slot)?;
        Ok(stack)
    }

    pub fn reclaim(&mut self, stack: NetworkStack) {
        self.recovered_slots.push(stack.reclaim());
    }
}
