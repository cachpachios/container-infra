use std::time::Duration;

use log::error;

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

    init::init();

    log::info!("Going into shell...");
    sh::cmd(&["sh"]);
}
