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
    delete_chain_args: Vec<Vec<String>>,
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
            delete_chain_args: Vec::new(),
        })
    }

    fn add_ip_rule(&mut self, args: &[&str], delete_args: Vec<String>) -> Result<()> {
        cmd("iptables-nft", args)?;
        self.delete_chain_args.push(delete_args);
        Ok(())
    }

    fn add_ip_rule_replace_first_arg(&mut self, args: &[&str], replaced_args: &str) -> Result<()> {
        // This function is stupid. Should be replace with direct nft commands instead of through iptables...
        let replace_arg_list = [replaced_args.to_string()]
            .into_iter()
            .chain(args.iter().skip(1).map(|s| s.to_string()))
            .collect::<Vec<String>>();
        self.add_ip_rule(args, replace_arg_list)
    }

    pub fn setup_public_nat(&mut self, outbound_if_name: &str) -> Result<()> {
        let addr = self.ipv4_addr.to_string();
        let nic_name = self.nic.name().to_owned(); // Stupid borrow checker
        self.add_ip_rule_replace_first_arg(
            &[
                "-A",
                "POSTROUTING",
                "-t",
                "nat",
                "-o",
                outbound_if_name,
                "-s",
                &addr,
                "-j",
                "MASQUERADE",
            ],
            "-D",
        )?;
        self.add_ip_rule_replace_first_arg(
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
            "-D",
        )?;

        self.add_ip_rule_replace_first_arg(
            &[
                "-A",
                "FORWARD",
                "-i",
                &nic_name,
                "-o",
                outbound_if_name,
                "-j",
                "ACCEPT",
            ],
            "-D",
        )?;
        Ok(())
    }

    pub fn setup_forwarding(
        &mut self,
        inbound_if_name: &str,
        inbound_port: u16,
        guest_port: u16,
    ) -> Result<()> {
        let nic_name: String = self.nic.name().to_owned(); // Stupid borrow checker
                                                           // DNAT outbound[host port] -> inbound[guest port]
        self.add_ip_rule_replace_first_arg(
            &[
                "-A",
                "PREROUTING",
                "-t",
                "nat",
                "-i",
                inbound_if_name,
                "-p",
                "tcp",
                "--dport",
                &inbound_port.to_string(),
                "-j",
                "DNAT",
                "--to-destination",
                &format!("{}:{}", self.ipv4_addr().to_string(), guest_port),
            ],
            "-D",
        )?;
        // SNAT inbound[guest port] -> outbound[host port]
        self.add_ip_rule_replace_first_arg(
            &[
                "-A",
                "POSTROUTING",
                "-t",
                "nat",
                "-o",
                nic_name.as_str(),
                "-p",
                "tcp",
                "--dport",
                &guest_port.to_string(),
                "-j",
                "MASQUERADE",
            ],
            "-D",
        )?;
        self.add_ip_rule_replace_first_arg(
            &[
                "-I",
                "FORWARD",
                "-i",
                nic_name.as_str(),
                "-o",
                inbound_if_name,
                "-p",
                "tcp",
                "--sport",
                &guest_port.to_string(),
                "-m",
                "state",
                "--state",
                "ESTABLISHED,RELATED",
                "-j",
                "ACCEPT",
            ],
            "-D",
        )?;
        self.add_ip_rule_replace_first_arg(
            &[
                "-I",
                "FORWARD",
                "-i",
                inbound_if_name,
                "-o",
                nic_name.as_str(),
                "-p",
                "tcp",
                "--dport",
                &guest_port.to_string(),
                "-m",
                "state",
                "--state",
                "NEW,ESTABLISHED,RELATED",
                "-j",
                "ACCEPT",
            ],
            "-D",
        )?;

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
        for rule in &self.delete_chain_args {
            let args = rule.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
            let _ = cmd("iptables-nft", args.as_slice());
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
