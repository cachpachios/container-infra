use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use hmac::Mac;
use log::debug;
use log::error;
use log::info;
use log::warn;
use proto::auth as proto_auth;
use proto::node::node_manager_server::NodeManager as NodeManagerService;
use proto::node::node_manager_server::NodeManagerServer as NodeManagerServiceServer;
use proto::node::AllLogs;
use proto::node::Empty;
use proto::node::InstanceId;
use proto::node::InstanceList;
use proto::node::LogMessage;
use proto::node::ProvisionRequest;
use proto::node::ProvisionResponse;
use proto::node::PublishServicePortRequest;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::metadata::MetadataMap;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use hmac::Hmac;
use sha2::Sha256;

use crate::machine;
use crate::machine::Machine;
use crate::networking::NetworkManager;

#[derive(Deserialize)]
pub struct ManagerConfig {
    pub firecracker_config: machine::FirecrackerConfig,
    pub public_network_interface: String,
    pub service_network_interface: String,
}

struct InnerNodeManager {
    config: ManagerConfig,
    machines: RwLock<HashMap<String, Machine>>,
    network: Mutex<NetworkManager>,
}

impl InnerNodeManager {
    async fn _provision(
        &self,
        request: ProvisionRequest,
        self_clone: Arc<InnerNodeManager>, // Self reference for cleanup
    ) -> anyhow::Result<String> {
        let mut machines = self.machines.write().await;

        let machine_config = machine::MachineConfig {
            container_reference: request.container_reference,
            vcpu_count: request.vcpus as u8,
            mem_size_mb: request.memory_mb as u32,
        };
        let mut network_stack = self.network.lock().await.provision_stack()?;
        debug!(
            "Network stack provision with local ip: {}",
            network_stack.ipv4_addr()
        );
        network_stack.setup_public_nat(&self.config.public_network_interface)?;

        let mut overrides = machine::ContainerOverrides {
            cmd_args: None,
            env: None,
        };

        if request.cmd_args.len() > 0 {
            overrides.cmd_args = Some(request.cmd_args);
        }
        if request.env.len() > 0 {
            overrides.env = Some(request.env.into_iter().collect());
        }

        let (machine, machine_stop_rx) = Machine::new(
            &self.config.firecracker_config,
            machine_config,
            network_stack,
            overrides,
        )
        .await?;
        let uuid = machine.uuid().to_string();
        info!("Provisioned node {} ", &uuid);
        machines.insert(uuid.clone(), machine);

        let uuid_clone = uuid.clone();

        // Cleanup task
        tokio::spawn(async move {
            if let Ok(_) = machine_stop_rx.await {
                let _ = self_clone._deprovision(&uuid_clone).await;
                info!("Machine {} stopped.", &uuid_clone);
            }
        });
        Ok(uuid)
    }

    async fn _deprovision(&self, id: &str) -> anyhow::Result<()> {
        let mut machines = self.machines.write().await;
        if let Some(machine) = machines.remove(id) {
            info!("Deprovisioning node {}", id);
            let mut network = self.network.lock().await;
            let network_stack = machine.shutdown().await;
            network.reclaim(network_stack);
        } else {
            warn!("Requested deprovisioning of missing machine with id {}", id);
        }
        Ok(())
    }

    async fn _drain(&self) -> anyhow::Result<()> {
        let mut machines = self.machines.write().await;
        let mut network_manager = self.network.lock().await;
        for (id, machine) in machines.drain() {
            info!("Deprovisioning id {}", &id);
            network_manager.reclaim(machine.shutdown().await);
        }
        Ok(())
    }
}

pub struct NodeManager {
    inner: Arc<InnerNodeManager>,
    authentication_secret: Option<Hmac<Sha256>>,
}

impl NodeManager {
    pub async fn new(
        config: ManagerConfig,
        authentication_secret: Option<&[u8]>,
    ) -> Result<(
        Self,
        tokio::sync::oneshot::Sender<tokio::sync::oneshot::Sender<()>>,
    )> {
        let inner = Arc::new(InnerNodeManager {
            machines: RwLock::new(HashMap::new()),
            config,
            network: Mutex::new(NetworkManager::new()?),
        });
        let (shutdown_tx, shutdown_rx) =
            tokio::sync::oneshot::channel::<tokio::sync::oneshot::Sender<()>>();

        let inner_clone = inner.clone();
        tokio::spawn(async move {
            if let Ok(finished) = shutdown_rx.await {
                warn!("Received shutdown signal, draining all machines");
                inner_clone._drain().await.unwrap_or_else(|e| {
                    error!("Failed to drain node manager: {}", e);
                });
                let _ = finished.send(());
            }
        });

        Ok((
            NodeManager {
                inner,
                authentication_secret: authentication_secret
                    .map(|s| Hmac::new_from_slice(s).expect("Inavlid secret key.")),
            },
            shutdown_tx,
        ))
    }

    fn validate_auth(
        &self,
        metadata: &MetadataMap,
        expected_audience: Option<&str>,
    ) -> Result<(), Status> {
        let secret = match self.authentication_secret {
            Some(ref secret) => secret,
            None => return Ok(()),
        };
        if !proto_auth::validate_authentication(
            metadata
                .get("auth")
                .and_then(|v| v.to_str().ok())
                .unwrap_or(""),
            secret,
            expected_audience,
        ) {
            return Err(Status::unauthenticated("Invalid authentication token"));
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl NodeManagerService for NodeManager {
    async fn provision(
        &self,
        request: Request<ProvisionRequest>,
    ) -> Result<Response<ProvisionResponse>, Status> {
        self.validate_auth(request.metadata(), None)?;
        let request = request.into_inner();
        debug!("Provisioning machine with request: {:?}", request);
        let id = self
            .inner
            ._provision(request, self.inner.clone())
            .await
            .map_err(|e| {
                error!("Failed to provision machine: {}", e);
                Status::internal("Failed to provision machine")
            })?;
        Ok(Response::new(ProvisionResponse { id }))
    }

    async fn deprovision(&self, request: Request<InstanceId>) -> Result<Response<Empty>, Status> {
        self.validate_auth(request.metadata(), Some(&request.get_ref().id))?;
        let request = request.into_inner();

        self.inner._deprovision(&request.id).await.map_err(|e| {
            error!("Failed to deprovision machine: {}", e);
            Status::internal("Failed to deprovision machine")
        })?;
        Ok(Response::new(Empty {}))
    }

    type StreamLogsStream = Pin<Box<dyn Stream<Item = Result<LogMessage, Status>> + Send>>;

    async fn stream_logs(
        &self,
        request: Request<InstanceId>,
    ) -> Result<Response<Self::StreamLogsStream>, Status> {
        self.validate_auth(request.metadata(), Some(&request.get_ref().id))?;
        let request = request.into_inner();
        let machines = self.inner.machines.read().await;
        let machine = match machines.get(&request.id) {
            Some(machine) => machine,
            None => {
                warn!("Requested logs for missing machine with id {}", &request.id);
                return Err(Status::not_found("Machine not found"));
            }
        };

        let (logs, mut log_rx) = machine.get_and_subscribe_to_logs().await.map_err(|e| {
            error!("Failed to get logs: {}", e);
            Status::internal("Failed to get logs")
        })?;

        let (tx, rpc_rx) = mpsc::channel(128);
        tokio::spawn(async move {
            for l in logs {
                let log_message = LogMessage {
                    message: l.text.clone(),
                    timestamp: l.timestamp_ms as i64,
                    log_type: l.message_type.as_str().to_string(),
                };
                if let Err(_) = tx.send(Ok(log_message)).await {
                    return;
                }
            }
            while let Some(l) = log_rx.recv().await {
                let log_message = LogMessage {
                    message: l.text.clone(),
                    timestamp: l.timestamp_ms as i64,
                    log_type: l.message_type.as_str().to_string(),
                };
                if let Err(_) = tx.send(Ok(log_message)).await {
                    return;
                }
            }
        });

        let output_stream = ReceiverStream::new(rpc_rx);
        Ok(Response::new(
            Box::pin(output_stream) as Self::StreamLogsStream
        ))
    }

    async fn list_instances(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<InstanceList>, Status> {
        self.validate_auth(request.metadata(), None)?;
        let machines = self.inner.machines.read().await;
        let instances = machines
            .iter()
            .map(|(id, _)| InstanceId { id: id.clone() })
            .collect::<Vec<_>>();
        Ok(Response::new(InstanceList { instances }))
    }

    async fn get_logs(&self, request: Request<InstanceId>) -> Result<Response<AllLogs>, Status> {
        self.validate_auth(request.metadata(), Some(&request.get_ref().id))?;
        let request = request.into_inner();
        let machines = self.inner.machines.read().await;
        let machine = match machines.get(&request.id) {
            Some(machine) => machine,
            None => {
                warn!("Requested logs for missing machine with id {}", &request.id);
                return Err(Status::not_found("Machine not found"));
            }
        };

        Ok(Response::new(AllLogs {
            logs: machine
                .get_logs()
                .await
                .map_err(|e| {
                    error!("Failed to get logs: {}", e);
                    Status::internal("Failed to get logs")
                })?
                .into_iter()
                .map(|s| LogMessage {
                    message: s.text.clone(),
                    timestamp: s.timestamp_ms as i64,
                    log_type: s.message_type.as_str().to_string(),
                })
                .collect(),
        }))
    }

    async fn publish_service_port(
        &self,
        request: Request<PublishServicePortRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.validate_auth(request.metadata(), Some(&request.get_ref().id))?;
        let request = request.into_inner();
        debug!("Publishing service port with request: {:?}", request);

        let host_port = u16::try_from(request.host_port)
            .map_err(|_| Status::invalid_argument("Invalid host port"))?;
        let guest_port = u16::try_from(request.guest_port)
            .map_err(|_| Status::invalid_argument("Invalid guest port"))?;

        let machines = self.inner.machines.read().await;
        let machine = match machines.get(&request.id) {
            Some(machine) => machine,
            None => {
                warn!(
                    "Requested publish port for missing machine with id {}",
                    &request.id
                );
                return Err(Status::not_found("Machine not found"));
            }
        };

        machine
            .network()
            .lock()
            .await
            .setup_forwarding(
                &self.inner.config.service_network_interface,
                host_port,
                guest_port,
            )
            .map_err(|e| {
                error!("Failed to publish service port: {}", e);
                Status::internal("Failed to publish service port")
            })?;

        Ok(Response::new(Empty {}))
    }

    async fn drain(&self, request: Request<Empty>) -> Result<Response<Empty>, Status> {
        self.validate_auth(request.metadata(), None)?;
        warn!("Draining all machines on node");
        self.inner._drain().await.map_err(|e| {
            error!("Failed to drain node: {}", e);
            Status::internal("Failed to drain node")
        })?;
        Ok(Response::new(Empty {}))
    }
}

pub async fn serve(
    manager: NodeManager,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = NodeManagerServiceServer::new(manager);
    info!("NodeManager server listening on {}", addr);
    tonic::transport::Server::builder()
        .add_service(server)
        .serve(addr)
        .await
        .expect("Failed to start server");
    Ok(())
}
