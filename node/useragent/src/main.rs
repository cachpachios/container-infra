use oci_spec::image;

mod containers;
mod init;
mod sh;

fn main() {
    simple_logger::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Info
    })
    .expect("Failed to initialize logger");

    log::info!("Running v. {}", env!("CARGO_PKG_VERSION"));

    let folder;
    // Skip init if env var NODEAGENT_DONT_INIT is set
    if std::env::var("NODEAGENT_DEBUG").is_err() {
        init::init();
        folder = std::path::PathBuf::from("/mnt");
    } else {
        log::info!("Skipping initialization");
        folder = std::env::current_dir().expect("Unable to get current directory");
    }

    let resource_name = "library/ubuntu";
    let reference = "ubuntu:24.04";
    let auth = containers::docker_io_oauth("repository", &resource_name, &["pull"])
        .expect("Unable to auth.");

    let root_folder = folder.join("rootfs");

    let (manifest, config) =
        match containers::pull_extract_image(&root_folder, reference, Some(&auth)) {
            Ok(res) => {
                log::info!("Image pulled and extracted successfully.");
                res
            }
            Err(e) => {
                log::error!("Error pulling image: {:?}", e);
                log::error!("Droping into shell");
                sh::cmd(&["sh"]);
                std::process::exit(0);
            }
        };

    if let Err(e) = config.to_file_pretty(folder.join("config.json")) {
        log::error!("Error writing config.json: {:?}", e);
    }

    log::info!("Droping into shell");
    sh::cmd(&["sh"]);
}
