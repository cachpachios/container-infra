use std::{
    collections::VecDeque,
    sync::{atomic::AtomicI32, Arc, Mutex},
};

use number_prefix::NumberPrefix;
use oci_spec::{distribution::Reference, runtime::Spec};
use rt::RuntimeOverrides;

use crate::{containers::registry::RegistryErrors, host::HostCommunication};

pub mod fs;
pub mod registry;
pub mod rt;

const CONCURRENT_LAYER_DOWNLOADS: usize = 5;

pub fn pull_and_prepare_image(
    reference: Reference,
    overrides: &RuntimeOverrides,
    comm: Arc<Mutex<HostCommunication>>,
) -> Result<Spec, registry::RegistryErrors> {
    comm.lock().unwrap().state_change(
        vmproto::guest::InitVmState::PullingContainerImage,
        Some(format!("Starting to pull container image.")),
    );

    let mut auth: Option<String> = None;

    if reference.registry() == "docker.io" {
        auth = Some(
            registry::docker_io_oauth("repository", &reference.repository(), &["pull"])
                .map_err(|_| registry::RegistryErrors::AuthenticationError)?,
        );
    }

    let folder = std::path::PathBuf::from("/mnt");

    let (manifest, config) = registry::get_manifest_and_config(&reference, auth.as_deref())?;

    let layers_folder = folder.join("layers");
    std::fs::create_dir_all(&layers_folder).map_err(|_| registry::RegistryErrors::IOErr)?;

    let layer_count = manifest.layers().len();
    comm.lock().unwrap().log_system_message(format!(
        "Image manifest fetched. Pulling {} layers... ",
        layer_count
    ));

    let worker_threads_count = CONCURRENT_LAYER_DOWNLOADS.min(layer_count);
    let mut worker_threads = Vec::with_capacity(worker_threads_count);

    let mut layer_folders = Vec::with_capacity(layer_count);

    for layer in manifest.layers() {
        let folder = layers_folder.join(layer.digest().to_string().replace(":", ""));
        std::fs::create_dir_all(&folder).map_err(|_| registry::RegistryErrors::IOErr)?;
        layer_folders.push(folder);
    }

    let layer_progress: Arc<AtomicI32> = Arc::new(0.into());

    let layers = Arc::new(Mutex::new(
        manifest.layers().iter().cloned().collect::<VecDeque<_>>(),
    ));

    for _ in 0..worker_threads_count {
        let reference = reference.clone();
        let auth = auth.clone();
        let comm = comm.clone();
        let progress = layer_progress.clone();
        let layers = layers.clone();
        let layers_folder = layers_folder.clone();
        let jh: std::thread::JoinHandle<Result<(), RegistryErrors>> =
            std::thread::spawn(move || {
                loop {
                    let layer = match layers.lock().unwrap().pop_front() {
                        Some(layer) => layer,
                        None => return Ok(()), // No more layers to process
                    };
                    let folder = layers_folder.join(layer.digest().to_string().replace(":", ""));
                    std::fs::create_dir_all(&folder)
                        .map_err(|_| registry::RegistryErrors::IOErr)?;
                    let layer_compressed_size = registry::pull_and_extract_layer(
                        &reference,
                        &layer,
                        &folder,
                        auth.as_deref(),
                    )?;

                    let layer_compressed_size =
                        match NumberPrefix::decimal(layer_compressed_size as f64) {
                            NumberPrefix::Standalone(v) => format!("{} bytes", v),
                            NumberPrefix::Prefixed(prefix, v) => format!("{} {}B", v, prefix),
                        };

                    let mut comm_clone_lock = comm.lock().unwrap();
                    let i = progress.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    comm_clone_lock.log_system_message(format!(
                        "Pulled and extracted layer {} of {} - {} ({})",
                        i + 1,
                        layer_count,
                        layer.digest(),
                        layer_compressed_size
                    ));
                }
            });
        worker_threads.push(jh);
    }

    // Wait for all threads to finish
    for jh in worker_threads {
        jh.join().map_err(|_| registry::RegistryErrors::IOErr)??;
    }

    config
        .to_file(&folder.join("image_config.json"))
        .expect("Unable to save config");

    let spec = rt::create_runtime_spec(&config, &overrides)
        .map_err(|_| registry::RegistryErrors::UnableToConstructRuntimeConfig)?;

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

    Ok(spec)
}
