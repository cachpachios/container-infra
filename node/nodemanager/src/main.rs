mod firecracker;
use std::path::Path;

use serde::Deserialize;

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

    let vm = firecracker::JailedCracker::new(jailer_bin, firecracker_bin, 0);

    vm.set_boot(kernel).expect("Unable to set boot source");
    vm.set_rootfs(rootfs).expect("Unable to set rootfs");

    println!("Starting VM");
    vm.start_vm().expect("Unable to start VM");
    std::thread::sleep(std::time::Duration::from_secs(20));
    vm.cleanup();
}
