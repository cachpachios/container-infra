use std::{
    fs::OpenOptions,
    panic::PanicHookInfo,
    process::{Command, Stdio},
};

use libc::reboot;
use oci_spec::distribution::Reference;

mod containers;
mod init;
mod mmds;
mod sh;

extern "C" fn handle_signal(sig: i32) {
    todo!("Gracefully handle signal {}", sig);
}

fn main() {
    simple_logger::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Info
    })
    .expect("Failed to initialize logger");

    std::panic::set_hook(Box::new(panic));

    if std::process::id() != 1 {
        panic!("This program is an init program and must be run as PID 1");
    }

    unsafe {
        libc::signal(libc::SIGINT, handle_signal as libc::sighandler_t);
    }

    log::info!("Running NodeAgent v. {}", env!("CARGO_PKG_VERSION"));

    init::init();

    let mmds = mmds::MMDSClient::connect().expect("Unable to connect to MMDS");

    #[derive(serde::Deserialize)]
    struct ContainerConfig {
        image: String,
    }

    let container_config: ContainerConfig = mmds
        .get("/latest/container")
        .expect("Unable to get container config");
    let reference = Reference::try_from(container_config.image).expect("Unable to parse reference");

    containers::pull_image(reference).expect("Unable to pull image");

    log::info!("Running container...");
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/ttyS0")
        .expect("Unable to open tty");

    // Clone for stdin, stdout, stderr
    let tty_in = tty.try_clone().expect("Unable to clone tty for stdin");
    let tty_out = tty.try_clone().expect("Unable to clone tty for stdout");
    let tty_err = tty.try_clone().expect("Unable to clone tty for stderr");

    let out = Command::new("/bin/crun")
        .arg("run")
        .arg("container")
        .current_dir("/mnt")
        .stdin(Stdio::from(tty_in))
        .stdout(Stdio::from(tty_out))
        .stderr(Stdio::from(tty_err))
        .spawn()
        .expect("Failed to spawn container")
        .wait_with_output()
        .expect("Unable to wait for container to exit.");

    log::info!("Container exited with code {}.", out.status);
    shutdown();
}

fn shutdown() {
    unsafe {
        reboot(libc::LINUX_REBOOT_CMD_RESTART);
    };
}

/// Panic handler
fn panic(info: &PanicHookInfo) {
    log::error!("Critical error occured during execution.");
    log::debug!("Panic: {}", info);
    log::debug!(
        "Panic location: {:?}",
        info.location().unwrap_or(&std::panic::Location::caller())
    );
    log::debug!(
        "Panic backtrace: {:?}",
        std::backtrace::Backtrace::force_capture()
    );
    log::debug!("Shutting down node...");
    shutdown();
}
