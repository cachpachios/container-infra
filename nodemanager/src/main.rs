use nodemanager::manager::{serve, ManagerConfig, NodeManager};

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("debug"));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();

    let config = std::fs::read_to_string("config.json").expect("Unable to read config file");
    let config: ManagerConfig = serde_json::from_str(&config).expect("Unable to parse config file");

    let addr = "[::1]:50051".parse().expect("Unable to parse address");

    rt.block_on(async {
        let (manager, shutdown_tx) = NodeManager::new(config, None, None)
            .await
            .expect("Unable to create NodeManager");

        tokio::spawn(async move { serve(manager, addr).await.expect("Unable to start server") });

        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C signal handler");
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        shutdown_tx
            .send(finish_tx)
            .expect("Failed to send shutdown signal");
        finish_rx
            .await
            .expect("Failed to wait for shutdown to finish");
    });
}
