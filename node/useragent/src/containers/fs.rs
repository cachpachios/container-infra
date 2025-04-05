use std::path::{Path, PathBuf};

use crate::sh::cmd;

pub fn create_overlay_fs<T: AsRef<Path>>(merged_path: T, work_path: T, layers: &Vec<PathBuf>) {
    cmd(&[
        "mount",
        "-t",
        "overlay",
        "overlay",
        "-o",
        &format!(
            "lowerdir={},upperdir={},workdir={}",
            layers
                .iter()
                .map(|layer| layer.to_str().unwrap())
                .collect::<Vec<&str>>()
                .join(":"),
            merged_path.as_ref().to_str().unwrap(),
            work_path.as_ref().to_str().unwrap()
        ),
        merged_path.as_ref().to_str().unwrap(),
    ]);
}
