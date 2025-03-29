use crate::sh::cmd;

pub fn init() {
    log::debug!("Mounting /proc");
    cmd(&["mount", "-t", "proc", "proc", "/proc"]);
    log::debug!("Mounting /sys");
    cmd(&["mount", "-t", "sysfs", "sysfs", "/sys"]);

    log::debug!("Creating FS in /dev/vdb");
    cmd(&["mkfs.ext2", "/dev/vdb"]);

    log::debug!("Mounting /dev/vdb");
    cmd(&["mount", "/dev/vdb", "/mnt"]);
}
