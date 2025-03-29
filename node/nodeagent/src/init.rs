use crate::sh::cmd;

pub fn init() {
    log::debug!("Mounting /proc");
    cmd(&["mount", "-t", "proc", "proc", "/proc"]);
    log::debug!("Mounting /sys");
    cmd(&["mount", "-t", "sysfs", "sysfs", "/sys"]);
}
