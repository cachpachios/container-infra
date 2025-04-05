use std::{
    fs::OpenOptions,
    os::fd::AsRawFd,
    process::{Command, Stdio},
};

use oci_spec::image;

mod containers;
mod init;
mod sh;

fn main() {
    simple_logger::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Info
    })
    .expect("Failed to initialize logger");

    log::info!("Running v. {}", env!("CARGO_PKG_VERSION"));

    let folder;
    // Skip init if env var NODEAGENT_DONT_INIT is set
    if std::env::var("NODEAGENT_DEBUG").is_err() {
        init::init();
        folder = std::path::PathBuf::from("/mnt");
    } else {
        log::info!("Skipping initialization");
        folder = std::env::current_dir().expect("Unable to get current directory");
    }

    // let resource_name = "library/ubuntu";
    // let reference = "ubuntu:24.04";
    // let auth = containers::docker_io_oauth("repository", &resource_name, &["pull"])
    //     .expect("Unable to auth.");

    // let root_folder = folder.join("rootfs");

    // let (manifest, config) =
    //     match containers::pull_extract_image(&root_folder, reference, Some(&auth)) {
    //         Ok(res) => {
    //             log::info!("Image pulled and extracted successfully.");
    //             res
    //         }
    //         Err(e) => {
    //             log::error!("Error pulling image: {:?}", e);
    //             log::error!("Droping into shell");
    //             sh::cmd(&["sh"]);
    //             std::process::exit(0);
    //         }
    //     };

    // let spec = containers::create_runtime_spec(config).expect("Unable to create runtime spec");

    // spec.save(&folder.join("config.json"))
    //     .expect("Unable to save runtime spec");

    log::info!("Droping into shell");
    unsafe {
        if libc::setsid() == -1 {
            eprintln!("setsid failed");
        }
    }

    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/ttyS0")
        .expect("Unable to open tty");

    // Set it as controlling terminal
    unsafe {
        libc::ioctl(tty.as_raw_fd(), libc::TIOCSCTTY, 0);
    }

    // Clone for stdin, stdout, stderr
    let tty_in = tty.try_clone().expect("Unable to clone tty for stdin");
    let tty_out = tty.try_clone().expect("Unable to clone tty for stdout");
    let tty_err = tty.try_clone().expect("Unable to clone tty for stderr");

    // Spawn a shell with the tty as stdio
    Command::new("/bin/sh")
        .stdin(Stdio::from(tty_in))
        .stdout(Stdio::from(tty_out))
        .stderr(Stdio::from(tty_err))
        .spawn()
        .expect("Failed to spawn shell")
        .wait()
        .expect("Unable to wait for shell to exit."); // wait for shell to exit
}
