pub fn cmd(cmd: &[&str]) {
    let output = std::process::Command::new("/sbin/busybox")
        .args(cmd)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start command");

    // Wait for the command to finish
    let output = output
        .wait_with_output()
        .expect("Failed to wait on command");

    if !output.status.success() {
        log::error!("Command {:?} failed with status: {}", cmd, output.status);
        panic!("Command failed: {:?}", cmd);
    } else {
        log::debug!("Command succeeded: {}", output.status);
    }
}
