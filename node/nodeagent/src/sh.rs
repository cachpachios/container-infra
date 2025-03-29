pub fn cmd(cmd: &[&str]) {
    let output = std::process::Command::new("/sbin/busybox")
        .args(cmd)
        .spawn()
        .expect("Failed to start command");

    // Wait for the command to finish
    let output = output
        .wait_with_output()
        .expect("Failed to wait on command");

    if !output.status.success() {
        log::error!("Command failed with status: {}", output.status);
    } else {
        log::debug!("Command succeeded: {}", output.status);
    }
}
