use crate::sh::cmd;

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
    cmd(&["mkfs.ext2", "/dev/vdb"]);

    log::debug!("Mounting /dev/vdb");
    cmd(&["mount", "/dev/vdb", "/mnt"]);
}
