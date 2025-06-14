use std::process::Command;

use crate::sh::cmd;

fn mke2fs(args: &[&str]) {
    let output = Command::new("/sbin/mke2fs")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start mke2fs");

    // Wait for the command to finish
    let output = output
        .wait_with_output()
        .expect("Failed to wait on command");

    if !output.status.success() {
        log::error!("Command failed with status: {}", output.status);
        panic!("Unable to create filesystem.");
    } else {
        log::debug!("Command succeeded: {}", output.status);
    }
}

pub fn init() {
    log::debug!("Mounting /proc");
    cmd(&["mount", "-t", "proc", "proc", "/proc"]);
    log::debug!("Mounting /sys");
    cmd(&["mount", "-t", "sysfs", "sysfs", "/sys"]);
    log::debug!("Mounting /run");
    cmd(&["mount", "-t", "tmpfs", "tmpfs", "/run"]);
    log::debug!("Mounting /var/run");
    cmd(&["mount", "-t", "tmpfs", "tmpfs", "/var/run"]);

    log::debug!("Creating and mounting /dev/pts");
    cmd(&["mkdir", "-p", "/dev/pts"]);
    cmd(&["mount", "-t", "devpts", "devpts", "/dev/pts"]);

    // Mount cgroup2
    log::debug!("Mounting /sys/fs/cgroup");
    cmd(&["mount", "-t", "cgroup2", "cgroup2", "/sys/fs/cgroup"]);

    // Creating R/W fs in /mnt
    log::debug!("Creating FS in /dev/vdb");
    mke2fs(&["-t", "ext4", "-O", "^has_journal", "/dev/vdb"]);

    log::debug!("Mounting /dev/vdb");
    cmd(&["mount", "/dev/vdb", "/mnt"]);

    // Setting session id
    log::debug!("Setting session id");
    let session_id: i32 = unsafe { libc::setsid() };
    if session_id < 0 {
        log::error!("Unable to set session id: {}", session_id);
    } else {
        log::debug!("Session id set to: {}", session_id);
    }

    // Enabling low level ports to be bound without root
    log::debug!("Enabling low level ports to be bound without root");
    cmd(&["sysctl", "-w", "net.ipv4.ip_unprivileged_port_start=0"]);

    // Enabling IP forwarding
    log::debug!("Enabling IP forwarding");
    cmd(&["sysctl", "-w", "net.ipv4.ip_forward=1"]);
}
