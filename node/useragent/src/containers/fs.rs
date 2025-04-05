use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum FSErrors {
    IOErr(&'static str),
}
struct ContainerFS {
    root_folder: PathBuf, // Root folder of the container
    layer_folder: PathBuf,
    download_folder: PathBuf, // Used to extract images etc to before move to the layer folder when done
}

pub fn prepare_filesystem(root_folder: &Path) -> Result<(), FSErrors> {
    let layer_folder = root_folder.join("layers");
    if !layer_folder.exists() {
        std::fs::create_dir(&layer_folder)
            .map_err(|_| FSErrors::IOErr("Unable to create layers folder"))?;
    }

    let rootfs = root_folder.join("rootfs");

    Ok(())
}
