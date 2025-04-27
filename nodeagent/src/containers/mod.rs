use oci_spec::distribution::Reference;
use rt::RuntimeOverrides;

pub mod fs;
pub mod registry;
pub mod rt;

pub fn pull_and_prepare_image(
    reference: Reference,
    overrides: &RuntimeOverrides,
) -> Result<(), registry::RegistryErrors> {
    log::info!("Pulling container image: {}", reference.whole(),);

    let auth = registry::docker_io_oauth("repository", &reference.repository(), &["pull"])
        .map_err(|_| registry::RegistryErrors::AuthenticationError)?;

    let folder = std::path::PathBuf::from("/mnt");

    let (manifest, config) = registry::get_manifest_and_config(&reference, Some(&auth))?;

    let layers_folder = folder.join("layers");
    std::fs::create_dir_all(&layers_folder).map_err(|_| registry::RegistryErrors::IOErr)?;

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
            std::fs::create_dir_all(&folder).map_err(|_| registry::RegistryErrors::IOErr)?;
            let r = registry::pull_and_extract_layer(&reference, &layer, &folder, Some(&auth));
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
        jh.join().map_err(|_| registry::RegistryErrors::IOErr)??;
    }

    config
        .to_file(&folder.join("image_config.json"))
        .expect("Unable to save config");

    let spec = rt::create_runtime_spec(&config, &overrides).expect("Unable to create runtime spec");

    spec.save(&folder.join("config.json"))
        .expect("Unable to save runtime spec");

    manifest
        .to_file_pretty(&folder.join("manifest.json"))
        .expect("Unable to save manifest");

    // Create the overlay filesystem
    let merged_path = folder.join("rootfs");
    let work_path = folder.join("work");
    std::fs::create_dir_all(&merged_path).map_err(|_| registry::RegistryErrors::IOErr)?;
    std::fs::create_dir_all(&work_path).map_err(|_| registry::RegistryErrors::IOErr)?;

    fs::create_overlay_fs(&merged_path, &work_path, &layer_folders);
    fs::prepare_fs(&merged_path).expect("Unable to prepare filesystem");

    log::info!("Image pulled and extracted successfully.");

    Ok(())
}
