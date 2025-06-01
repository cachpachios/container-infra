use std::collections::HashMap;
use std::fmt::Debug;

use chrono::DateTime;
use log::error;
use log::info;
use proto::node::DeprovisionRequest;
use proto::node::Empty;
use proto::node::InstanceId;
use proto::node::ProvisionRequest;
use proto::node::node_manager_client::NodeManagerClient;

use clap::{Parser, Subcommand};
use proto::node::PublishServicePortRequest;

#[derive(Debug, Parser)]
#[command(name = "nodecli")]
#[command(about = "CLI for interacting directly with NodeManagers", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long)]
    address: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(arg_required_else_help = true)]
    Run {
        container_reference: String,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=32), help="Number of vCPUs to provision, max 32.")]
        vcpus: u8,
        #[arg(long, default_value_t = 1024, help = "Memory in MB")]
        memory_mb: u32,

        #[arg(
            long,
            default_value_t = false,
            help = "Don't tail logs after provisioning"
        )]
        dont_tail_logs: bool,

        #[arg(short, long, help = "Environment variables to set in the container")]
        environment: Option<Vec<String>>,

        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            help = "Command line arguments to override when running the container"
        )]
        args: Vec<String>,
    },
    #[command(arg_required_else_help = true)]
    Rm {
        #[arg(help = "Instance UUID")]
        instance_id: String,
    },
    #[command(arg_required_else_help = false)]
    Ls,
    Logs {
        #[arg(help = "Instance UUID")]
        instance_id: String,

        #[arg(long, default_value_t = false)]
        tail: bool,
    },
    Pub {
        #[arg(help = "Instance UUID")]
        instance_id: String,

        #[arg(help = "Port to publish on the host")]
        host_port: u16,
        #[arg(help = "Port to route to in the container")]
        guest_port: u16,
    },
    Drain,
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
    let mut client =
        NodeManagerClient::connect(cli.address.unwrap_or("http://[::1]:50051".to_string())).await?;

    match cli.command {
        Commands::Run {
            container_reference,
            vcpus,
            memory_mb,
            dont_tail_logs,
            environment,
            args,
        } => {
            let mut parsed_env =
                HashMap::with_capacity(environment.as_ref().map_or(0, |v| v.len()));
            for env in environment.unwrap_or_default() {
                let mut parts = env.split('=');
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    parsed_env.insert(key.to_string(), value.to_string());
                } else {
                    error!(
                        "Invalid environment variable format: {}, expected \"KEY=VALUE\".",
                        env
                    );
                }
            }

            let request = tonic::Request::new(ProvisionRequest {
                container_reference,
                vcpus: vcpus as i32,
                memory_mb: memory_mb as i32,
                env: parsed_env,
                cmd_args: args,
            });

            let response = client.provision(request).await;
            match response {
                Ok(res) => {
                    let instance_id = res.into_inner().id;
                    info!("Provisioned instance with id {}", instance_id);
                    if !dont_tail_logs {
                        stream_logs(&mut client, instance_id).await;
                    }
                }
                Err(e) => error!("Failed to provision instance: {}", e),
            }
        }
        Commands::Rm { instance_id } => {
            let request = tonic::Request::new(DeprovisionRequest {
                instance_id: instance_id.clone(),
                timeout_millis: 5000, // Default timeout of 5 seconds
            });
            let response = client.deprovision(request).await;
            match response {
                Ok(_) => info!("Deprovisioned instance with id {}", instance_id),
                Err(e) => error!("Failed to deprovision instance: {}", e),
            }
        }
        Commands::Ls => {
            let request = tonic::Request::new(Empty {});
            let response = client.list_instances(request).await;
            match response {
                Ok(res) => {
                    let instances = res.into_inner().instances;
                    for instance in instances {
                        println!("{}", instance.id);
                    }
                }
                Err(e) => error!("Failed to list instances: {}", e),
            }
        }
        Commands::Logs { instance_id, tail } => {
            if tail {
                stream_logs(&mut client, instance_id.clone()).await;
            } else {
                client
                    .get_logs(tonic::Request::new(InstanceId {
                        id: instance_id.clone(),
                    }))
                    .await
                    .map(|response| {
                        let logs = response.into_inner().logs;
                        for log in logs {
                            let ts = DateTime::from_timestamp(
                                log.timestamp / 1000,
                                (log.timestamp as u32 % 1000) * 1_000_000,
                            )
                            .unwrap()
                            .with_timezone(&chrono::Local);
                            println!(
                                "[{}] {} - {}",
                                log.log_type,
                                ts.format("%Y-%m-%d %H:%M:%S%.3f"),
                                log.message
                            );
                        }
                    })
                    .map_err(|e| error!("Failed to get logs for instance {}: {}", instance_id, e))
                    .ok();
            }
        }
        Commands::Pub {
            instance_id,
            guest_port,
            host_port,
        } => {
            let request = tonic::Request::new(PublishServicePortRequest {
                id: instance_id.clone(),
                guest_port: guest_port as i32,
                host_port: host_port as i32,
            });
            let response = client.publish_service_port(request).await;
            match response {
                Ok(_) => info!(
                    "Published port {} on instance {} to host port {}",
                    guest_port, instance_id, host_port
                ),
                Err(e) => error!("Failed to publish port: {}", e),
            }
        }
        Commands::Drain => {
            if let Err(e) = client.drain(tonic::Request::new(Empty {})).await {
                error!("Failed to drain: {}", e);
            } else {
                info!("Drained node!");
            }
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
                        let ts = DateTime::from_timestamp(
                            log_message.timestamp / 1000,
                            (log_message.timestamp as u32 % 1000) * 1_000_000,
                        )
                        .unwrap()
                        .with_timezone(&chrono::Local);
                        println!(
                            "[{}] {} - {}",
                            log_message.log_type,
                            ts.format("%Y-%m-%d %H:%M:%S%.3f"),
                            log_message.message
                        );
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
