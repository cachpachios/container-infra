use std::{
    fs::OpenOptions,
    os::fd::AsRawFd,
    path::PathBuf,
    process::{Command, Stdio},
};

use oci_spec::distribution::Reference;

mod containers;
mod init;
mod sh;

fn pull_image() -> Result<(), containers::registry::RegistryErrors> {
    let resource_name = "library/ubuntu";
    let reference = Reference::try_from("ubuntu:24.04").expect("Unable to parse reference");

    log::info!("Pulling container image: {}", reference);

    let auth = containers::registry::docker_io_oauth("repository", &resource_name, &["pull"])
        .map_err(|_| containers::registry::RegistryErrors::AuthenticationError)?;
    let folder = PathBuf::from("/mnt");

    let (manifest, config) =
        containers::registry::get_manifest_and_config(&reference, Some(&auth))?;

    let layers_folder = folder.join("layers");
    std::fs::create_dir_all(&layers_folder).map_err(|_| {
        containers::registry::RegistryErrors::IOErr("Unable to create layers folder")
    })?;

    let layer_count = manifest.layers().len();
    let mut layer_threads = Vec::with_capacity(layer_count);

    let mut layer_folders = Vec::with_capacity(layer_count);

    for layer in manifest.layers().iter() {
        let layer = layer.clone();
        let reference = reference.clone();
        let folder = layers_folder.join(layer.digest().to_string().replace(":", ""));
        layer_folders.push(folder.clone());

        let auth = auth.clone();

        // Spawn a thread to pull the layer
        let jh = std::thread::spawn(move || {
            std::fs::create_dir_all(&folder).map_err(|_| {
                containers::registry::RegistryErrors::IOErr("Unable to create layer folder")
            })?;
            containers::registry::pull_and_extract_layer(&reference, &layer, &folder, Some(&auth))
        });
        layer_threads.push(jh);
    }

    // Wait for all threads to finish
    for jh in layer_threads {
        jh.join().map_err(|_| {
            containers::registry::RegistryErrors::IOErr("Unable to join thread for layer")
        })??;
    }

    let overrides = containers::rt::RuntimeOverrides {
        args: None,
        terminal: true,
    };

    let spec = containers::rt::create_runtime_spec(&config, &overrides)
        .expect("Unable to create runtime spec");

    spec.save(&folder.join("config.json"))
        .expect("Unable to save runtime spec");

    manifest
        .to_file_pretty(&folder.join("manifest.json"))
        .expect("Unable to save manifest");

    // Create the overlay filesystem
    let merged_path = folder.join("rootfs");
    let work_path = folder.join("work");
    std::fs::create_dir_all(&merged_path).map_err(|_| {
        containers::registry::RegistryErrors::IOErr("Unable to create merged folder")
    })?;
    std::fs::create_dir_all(&work_path)
        .map_err(|_| containers::registry::RegistryErrors::IOErr("Unable to create work folder"))?;

    containers::fs::create_overlay_fs(&merged_path, &work_path, &layer_folders);
    containers::fs::prepare_fs(&merged_path);

    log::info!("Image pulled and extracted successfully.");

    Ok(())
}

fn main() {
    simple_logger::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Info
    })
    .expect("Failed to initialize logger");

    log::info!("Running v. {}", env!("CARGO_PKG_VERSION"));

    init::init();

    loop {
        let r = pull_image();
        match r {
            Ok(_) => {
                break;
            }
            Err(e) => {
                log::error!("Error pulling image: {:?}", e);
                log::error!("Retrying in 5 seconds...");
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }

    log::info!("Running container...");
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/ttyS0")
        .expect("Unable to open tty");

    // Clone for stdin, stdout, stderr
    let tty_in = tty.try_clone().expect("Unable to clone tty for stdin");
    let tty_out = tty.try_clone().expect("Unable to clone tty for stdout");
    let tty_err = tty.try_clone().expect("Unable to clone tty for stderr");

    Command::new("/bin/crun")
        .arg("run")
        .arg("container")
        .current_dir("/mnt")
        .stdin(Stdio::from(tty_in))
        .stdout(Stdio::from(tty_out))
        .stderr(Stdio::from(tty_err))
        .spawn()
        .expect("Failed to spawn container")
        .wait()
        .expect("Unable to wait for container to exit.");
}
