use std::{
    collections::BTreeMap,
    io::Write,
    panic::PanicHookInfo,
    process::Command,
    sync::{Arc, Mutex},
    thread::sleep,
};

use libc::{reboot, sync};
use oci_spec::distribution::Reference;
use vmproto::guest::{GuestExitCode, GuestPacket, LogMessageType};

mod containers;
mod host;
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

    let mut mmds = mmds::MMDSClient::connect().expect("Unable to connect to MMDS");

    #[derive(serde::Deserialize)]
    struct Config {
        image: String,
        cmd_args: Option<Vec<String>>,
        env: Option<BTreeMap<String, String>>,
        vsock_port: u32,
    }

    let config: Config = mmds
        .get("/latest/container")
        .expect("Unable to get container config");

    let comm = Arc::new(Mutex::new(
        host::HostCommunication::new(config.vsock_port as u32)
            .expect("Unable to connect to host communication channel"),
    ));

    comm.lock()
        .unwrap()
        .log_system_message(format!("NodeAgent v. {}", env!("CARGO_PKG_VERSION")));

    let reference = match Reference::try_from(config.image) {
        Ok(reference) => reference,
        Err(e) => {
            log::error!("Unable to parse container image reference: {}", e);
            shutdown();
            return;
        }
    };

    let rt_overrides = crate::containers::rt::RuntimeOverrides {
        additional_args: config.cmd_args,
        additional_env: config.env,
        terminal: false,
    };

    comm.lock()
        .unwrap()
        .log_system_message(format!("Pulling container image: {}", reference));

    if let Err(r) = containers::pull_and_prepare_image(reference, &rt_overrides) {
        log::error!("Unable to pull and extract container image: {:?}", r);
        comm.lock()
            .unwrap()
            .log_system_message(format!("Failed to pull container image: {:?}", r));
        comm.lock()
            .unwrap()
            .write(GuestPacket::Exited(
                GuestExitCode::FailedToPullContainerImage,
            ))
            .expect("Failed to write exit status to host");
        shutdown();
        return;
    }

    log::info!("Running container...");
    log::debug!("Runtime overrides: {:?}", rt_overrides);
    flush_buffers();

    comm.lock()
        .unwrap()
        .log_system_message("Executing container...".to_string());

    let mut out = Command::new("/bin/crun")
        .arg("run")
        .arg("container")
        .current_dir("/mnt")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn container");

    let stdout = out.stdout.take().expect("Failed to get stdout");
    let stderr = out.stderr.take().expect("Failed to get stderr");

    let stdout_thread =
        host::spawn_pipe_to_log(comm.clone(), Box::new(stdout), LogMessageType::Stdout);
    let stderr_thread =
        host::spawn_pipe_to_log(comm.clone(), Box::new(stderr), LogMessageType::Stderr);

    let res = out.wait().expect("Failed to wait for container process");

    comm.lock()
        .unwrap()
        .write(GuestPacket::Exited(GuestExitCode::ContainerExited(
            res.code().unwrap_or(9993),
        )))
        .expect("Failed to write exit status to host");

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    log::info!("Container exited with status: {:?}", res.code());
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
