use log::error;
use log::info;
use proto::node::InstanceId;
use proto::node::ProvisionRequest;
use proto::node::node_manager_client::NodeManagerClient;
use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    Provision { container_reference: String },
    #[command(arg_required_else_help = true)]
    Deprovision { instance_id: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let cli = Cli::parse();

    let mut client = NodeManagerClient::connect("http://[::1]:50051").await?;

    match cli.command {
        Commands::Provision {
            container_reference,
        } => {
            let request = tonic::Request::new(ProvisionRequest {
                container_reference,
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
    }
    Ok(())
}
