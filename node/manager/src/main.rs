use serde::Deserialize;
use std::{
    io::{Read, Write},
    path::Path,
};

mod firecracker;
mod networking;

#[derive(Deserialize)]
struct Config {
    firecracker_binary: String,
    jailer_binary: String,
    kernel_image: String,
    rootfs: String,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let config = std::fs::read_to_string("config.json").expect("Unable to read config file");
    let config: Config = serde_json::from_str(&config).expect("Unable to parse config file");

    let rootfs = Path::new(&config.rootfs);
    let kernel = Path::new(&config.kernel_image);
    let jailer_bin = Path::new(&config.jailer_binary);
    let firecracker_bin = Path::new(&config.firecracker_binary);

    let metadata = "{
        \"latest\": {
            \"container\": {\"image\": \"nginx:latest\"}
        }
    }";

    let (mut vm, mut out) =
        firecracker::JailedCracker::spawn(jailer_bin, firecracker_bin, 0, Some(metadata))
            .await
            .expect("Unable to spawn firecracker");
    std::thread::spawn(move || {
        let mut buf = [0; 1024];
        let mut our = std::io::stdout();
        loop {
            match out.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    our.write_all(&buf[..n]).expect("Unable to write to stdout");
                }
                Err(_) => {
                    break;
                }
            }
        }
    });

    vm.set_machine_config(4u8, 1024u32)
        .await
        .expect("Unable to set machine config");
    vm.set_boot(
        kernel,
        "console=ttyS0 quiet loglevel=1 reboot=k panic=-1 pci=off ip=172.16.0.2::172.16.0.1:255.255.255.252::eth0:off",
    )
    .await
    .expect("Unable to set boot source");
    vm.set_rootfs(rootfs).await.expect("Unable to set rootfs");
    vm.create_drive(10, "drive0")
        .await
        .expect("Unable to create drive");

    let tap = networking::TunTap::new("tap0").expect("Unable to create tap device");
    tap.add_address("172.16.0.1/30")
        .expect("Unable to add address to tap device");
    tap.up().expect("Unable to bring up tap device");

    vm.set_eth_tap(&tap)
        .await
        .expect("Unable to add tap device");

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

    vm.start_vm().await.expect("Unable to start VM");
    vm.wait();

    vm.cleanup().expect("Cleanup failed");
}
