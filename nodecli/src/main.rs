use log::error;
use log::info;
use proto::node::InstanceId;
use proto::node::ProvisionRequest;
use proto::node::node_manager_client::NodeManagerClient;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "nodecli")]
#[command(about = "CLI for interacting directly with NodeManagers", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(arg_required_else_help = true)]
    Provision {
        container_reference: String,
        #[arg(long, default_value_t = 1)]
        vcpus: u8,
        #[arg(long, default_value_t = 1024)]
        memory_mb: u32,
    },
    #[command(arg_required_else_help = true)]
    Deprovision {
        instance_id: String,
    },
    Log {
        instance_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    simple_logger::init_with_level(if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Info
    })
    .expect("Failed to initialize logger");

    let cli = Cli::parse();

    let mut client = NodeManagerClient::connect("http://[::1]:50051").await?;

    match cli.command {
        Commands::Provision {
            container_reference,
            vcpus,
            memory_mb,
        } => {
            let request = tonic::Request::new(ProvisionRequest {
                container_reference,
                vcpus: vcpus as i32,
                memory_mb: memory_mb as i32,
            });

            let response = client.provision(request).await;
            match response {
                Ok(res) => {
                    let instance_id = res.into_inner().id;
                    info!("Provisioned instance with id {}", instance_id);
                }
                Err(e) => error!("Failed to provision instance: {}", e),
            }
        }
        Commands::Deprovision { instance_id } => {
            let request = tonic::Request::new(InstanceId {
                id: instance_id.clone(),
            });
            let response = client.deprovision(request).await;
            match response {
                Ok(_) => info!("Deprovisioned instance with id {}", instance_id),
                Err(e) => error!("Failed to deprovision instance: {}", e),
            }
        }
        Commands::Log { instance_id } => {
            stream_logs(&mut client, instance_id.clone()).await;
        }
    }
    Ok(())
}

async fn stream_logs(
    client: &mut NodeManagerClient<tonic::transport::Channel>,
    instance_id: String,
) {
    let request = tonic::Request::new(InstanceId {
        id: instance_id.clone(),
    });
    let response = client.stream_logs(request).await;
    match response {
        Ok(stream) => {
            let mut stream = stream.into_inner();
            loop {
                let next = stream.message().await;
                match next {
                    Ok(Some(log_message)) => {
                        println!("{}", log_message.message);
                    }
                    Ok(None) => {
                        info!("No more logs for instance {}", instance_id);
                        break;
                    }
                    Err(e) => {
                        error!("Error receiving log message: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => error!("Failed to get logs for instance {}: {}", instance_id, e),
    }
}
