use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::Write,
    panic::PanicHookInfo,
    process::{Command, Stdio},
    thread::sleep,
};

use libc::{reboot, sync};
use oci_spec::distribution::Reference;

mod containers;
mod init;
mod mmds;
mod sh;

extern "C" fn handle_signal(sig: i32) {
    log::info!("Received signal {}", sig);
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
        cmd_args: Option<Vec<String>>,
        env: Option<BTreeMap<String, String>>,
    }

    let container_config: ContainerConfig = mmds
        .get("/latest/container")
        .expect("Unable to get container config");
    let reference = match Reference::try_from(container_config.image) {
        Ok(reference) => reference,
        Err(e) => {
            log::error!("Unable to parse container image reference: {}", e);
            shutdown();
            return;
        }
    };

    let rt_overrides = crate::containers::rt::RuntimeOverrides {
        additional_args: container_config.cmd_args,
        additional_env: container_config.env,
        terminal: false,
    };

    if let Err(r) = containers::pull_and_prepare_image(reference, &rt_overrides) {
        log::error!("Unable to pull and extract container image: {:?}", r);
        shutdown();
        return;
    }

    log::info!("Running container...");
    log::debug!("Runtime overrides: {:?}", rt_overrides);
    flush_buffers();

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

fn flush_buffers() {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

fn shutdown() {
    log::info!("Shutting down...");
    flush_buffers();
    if cfg!(debug_assertions) {
        // Sleep to ensure logs are flushed
        sleep(std::time::Duration::from_millis(100));
    }
    unsafe {
        sync();
        reboot(libc::LINUX_REBOOT_CMD_RESTART);
        std::process::exit(1);
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
    flush_buffers();
    shutdown();
}
