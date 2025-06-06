use std::{
    collections::BTreeMap,
    io::Write,
    panic::PanicHookInfo,
    process::Command,
    sync::{Arc, Mutex},
    thread::sleep,
};

use host::read_packet;
use libc::{reboot, sync};
use oci_spec::distribution::Reference;
use vmproto::{
    guest::{GuestExitCode, InitVmState, LogMessageType},
    host::HostPacket,
};

mod containers;
mod host;
mod init;
mod mmds;
mod sh;

fn main() {
    simple_logger::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Error
    })
    .expect("Failed to initialize logger");

    std::panic::set_hook(Box::new(panic));

    if std::process::id() != 1 {
        panic!("This program is an init program and must be run as PID 1");
    }

    log::info!("Running Instance v. {}", env!("CARGO_PKG_VERSION"));

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

    comm.lock().unwrap().state_change(
        InitVmState::Online,
        Some(format!("Instance v. {}", env!("CARGO_PKG_VERSION"))),
    );

    let (exit_tx, exit_rx) = std::sync::mpsc::channel();

    let container_running = Arc::new(Mutex::new(false));
    let mut read_stream = comm
        .lock()
        .unwrap()
        .clone_stream()
        .expect("Failed to clone stream");
    let host_requested_shutdown_tx = exit_tx.clone();
    std::thread::spawn(move || loop {
        let packet = read_packet(&mut read_stream).expect("Failed to read packet from host");
        match packet {
            HostPacket::Shutdown => {
                log::info!("Received shutdown command from host");
                host_requested_shutdown_tx
                    .send(GuestExitCode::GracefulShutdown)
                    .expect("Failed to send shutdown exit code");
            }
        }
    });

    let reference = match Reference::try_from(config.image) {
        Ok(reference) => reference,
        Err(e) => {
            log::error!("Unable to parse container image reference: {}", e);
            comm.lock().unwrap().exit(
                GuestExitCode::FailedToPullContainerImage,
                Some(format!("Failed to parse container image reference: {}", e)),
            );
            shutdown();
            return;
        }
    };

    let rt_overrides = crate::containers::rt::RuntimeOverrides {
        additional_args: config.cmd_args,
        additional_env: config.env,
        terminal: false,
    };

    if let Err(r) = containers::pull_and_prepare_image(reference, &rt_overrides, comm.clone()) {
        log::error!("Unable to pull and extract container image: {:?}", r);
        comm.lock().unwrap().exit(
            GuestExitCode::FailedToPullContainerImage,
            Some(format!(
                "Unable to pull and extract container image: {:?}",
                r
            )),
        );
        shutdown();
        return;
    }

    log::info!("Running container...");
    log::debug!("Runtime overrides: {:?}", rt_overrides);
    flush_buffers();

    comm.lock()
        .unwrap()
        .state_change(InitVmState::ExecutingContainer, None);

    *container_running.lock().unwrap() = true;

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

    let container_exit_tx = exit_tx.clone();
    let comm_clone = comm.clone();
    std::thread::spawn(move || {
        let res = out.wait().expect("Failed to wait for container process");
        comm_clone.lock().unwrap().log_system_message(format!(
            "Container exited with code: {}",
            res.code()
                .map(|c| c.to_string())
                .unwrap_or("unknown".to_string())
        ));
        container_exit_tx.send(GuestExitCode::ContainerExited(res.code().unwrap_or(9999)))
    });

    let mut res = exit_rx
        .recv()
        .expect("Failed to receive exit status from container process");
    log::info!("Exited recieved: {:?}", res);

    if res == GuestExitCode::GracefulShutdown {
        comm.lock().unwrap().log_system_message(
            "Received graceful shutdown command... Stopping container.".to_string(),
        );

        if let Ok(mut stop_cmd) = Command::new("/bin/crun")
            .arg("kill")
            .arg("container")
            .current_dir("/mnt")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            let _ = stop_cmd.wait();
        }

        // wait for the graceful shutdown to complete
        res = exit_rx
            .recv()
            .expect("Failed to receive exit status from container process after shutdown");
    }
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    comm.lock().unwrap().exit(res, None);

    log::info!("Container exited with status: {:?}", res);
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
