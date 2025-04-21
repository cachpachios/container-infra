use manager::NodeManager;

mod machine;
mod manager;

fn main() {
    env_logger::init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();

    let config = std::fs::read_to_string("config.json").expect("Unable to read config file");
    let config: machine::FirecrackerConfig =
        serde_json::from_str(&config).expect("Unable to parse config file");

    let manager = NodeManager::new(config);

    rt.block_on(async {
        manager::run_server(manager)
            .await
            .expect("Unable to start server");
    });
}
