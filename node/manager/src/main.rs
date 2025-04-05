use serde::Deserialize;
use std::path::Path;

mod firecracker;
mod networking;

#[derive(Deserialize)]
struct Config {
    firecracker_binary: String,
    jailer_binary: String,
    kernel_image: String,
    rootfs: String,
}

fn main() {
    env_logger::init();

    let config = std::fs::read_to_string("config.json").expect("Unable to read config file");
    let config: Config = serde_json::from_str(&config).expect("Unable to parse config file");

    let rootfs = Path::new(&config.rootfs);
    let kernel = Path::new(&config.kernel_image);
    let jailer_bin = Path::new(&config.jailer_binary);
    let firecracker_bin = Path::new(&config.firecracker_binary);

    let mut vm = firecracker::JailedCracker::new(jailer_bin, firecracker_bin, 0);

    vm.set_boot(
        kernel,
        "reboot=k panic=1 pci=off ip=172.16.0.2::172.16.0.1:255.255.255.252::eth0:off",
    )
    .expect("Unable to set boot source");
    vm.set_rootfs(rootfs).expect("Unable to set rootfs");
    vm.create_drive(10, "drive0")
        .expect("Unable to create drive");

    let tap = networking::TunTap::new("tap0").expect("Unable to create tap device");
    tap.add_address("172.16.0.1/30")
        .expect("Unable to add address to tap device");
    tap.up().expect("Unable to bring up tap device");

    vm.set_eth_tap(&tap).expect("Unable to add tap device");

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
    )
    .expect("Unable to add iptables rule");
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
    )
    .expect("Unable to add iptables rule");

    networking::cmd(
        "iptables-nft",
        &["-A", "FORWARD", "-i", &tap.name, "-o", ours, "-j", "ACCEPT"],
    )
    .expect("Unable to add iptables rule");

    vm.start_vm().expect("Unable to start VM");

    vm.wait();
    vm.cleanup();
}
