use std::{
    fs::OpenOptions,
    path::PathBuf,
    process::{Command, Stdio},
};

use libc::reboot;
use oci_spec::distribution::Reference;

mod containers;
mod init;
mod sh;

fn pull_image() -> Result<(), containers::registry::RegistryErrors> {
    let reference = Reference::try_from("nginx:latest").expect("Unable to parse reference");

    log::info!("Pulling container image: {}", reference.whole(),);

    let auth =
        containers::registry::docker_io_oauth("repository", &reference.repository(), &["pull"])
            .map_err(|_| containers::registry::RegistryErrors::AuthenticationError)?;

    let folder = PathBuf::from("/mnt");

    let (manifest, config) =
        containers::registry::get_manifest_and_config(&reference, Some(&auth))?;

    let layers_folder = folder.join("layers");
    std::fs::create_dir_all(&layers_folder)
        .map_err(|_| containers::registry::RegistryErrors::IOErr)?;

    let layer_count = manifest.layers().len();
    let mut layer_threads = Vec::with_capacity(layer_count);

    let mut layer_folders = Vec::with_capacity(layer_count);

    for (i, layer) in manifest.layers().iter().enumerate() {
        log::info!(
            "Pulling layer {} of {} - {}",
            i + 1,
            layer_count,
            layer.digest()
        );
        let layer = layer.clone();
        let reference = reference.clone();
        let folder = layers_folder.join(layer.digest().to_string().replace(":", ""));
        layer_folders.push(folder.clone());

        let auth = auth.clone();

        // Spawn a thread to pull the layer
        let jh = std::thread::spawn(move || {
            std::fs::create_dir_all(&folder)
                .map_err(|_| containers::registry::RegistryErrors::IOErr)?;
            let r = containers::registry::pull_and_extract_layer(
                &reference,
                &layer,
                &folder,
                Some(&auth),
            );
            log::info!(
                "Pulled layer {} of {} - {}",
                i + 1,
                layer_count,
                layer.digest()
            );
            r
        });
        layer_threads.push(jh);
    }

    // Wait for all threads to finish
    for jh in layer_threads {
        jh.join()
            .map_err(|_| containers::registry::RegistryErrors::IOErr)??;
    }

    let overrides = containers::rt::RuntimeOverrides {
        args: None, //Some(vec!["/bin/sh".to_string()]),
        terminal: true,
    };

    config
        .to_file(&folder.join("image_config.json"))
        .expect("Unable to save config");

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
    std::fs::create_dir_all(&merged_path)
        .map_err(|_| containers::registry::RegistryErrors::IOErr)?;
    std::fs::create_dir_all(&work_path).map_err(|_| containers::registry::RegistryErrors::IOErr)?;

    containers::fs::create_overlay_fs(&merged_path, &work_path, &layer_folders);
    containers::fs::prepare_fs(&merged_path).expect("Unable to prepare filesystem");

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

    log::info!("Running NodeAgent v. {}", env!("CARGO_PKG_VERSION"));

    init::init();

    pull_image().expect("Unable to pull image");

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

    log::info!("Container exited, shutting down...");
    unsafe {
        reboot(libc::LINUX_REBOOT_CMD_RESTART);
    };
}
