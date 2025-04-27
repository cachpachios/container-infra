use std::collections::HashMap;
use std::pin::Pin;

use log::debug;
use log::error;
use log::info;
use log::warn;
use proto::node::node_manager_server::NodeManager as NodeManagerService;
use proto::node::node_manager_server::NodeManagerServer as NodeManagerServiceServer;
use proto::node::AllLogs;
use proto::node::Empty;
use proto::node::InstanceId;
use proto::node::LogMessage;
use proto::node::ProvisionRequest;
use proto::node::ProvisionResponse;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use crate::machine;
use crate::machine::Machine;
use crate::networking::NetworkManager;

pub struct NodeManager {
    machines: RwLock<HashMap<String, Machine>>,
    fc_config: machine::ManagerConfig,
    network: Mutex<NetworkManager>,
}

impl NodeManager {
    pub fn new(fc_config: machine::ManagerConfig) -> Self {
        NodeManager {
            machines: RwLock::new(HashMap::new()),
            fc_config,
            network: Mutex::new(NetworkManager::new()),
        }
    }
}

#[tonic::async_trait]
impl NodeManagerService for NodeManager {
    async fn provision(
        &self,
        request: Request<ProvisionRequest>,
    ) -> Result<Response<ProvisionResponse>, Status> {
        let request = request.into_inner();

        let mut machines = self.machines.write().await;

        let machine_config = machine::MachineConfig {
            container_reference: request.container_reference,
            vcpu_count: request.vcpus as u8,
            mem_size_mb: request.memory_mb as u32,
        };
        let mut network_stack = self.network.lock().await.provision_stack().map_err(|e| {
            error!("Failed to create network stack: {}", e);
            Status::internal("Failed to create network stack")
        })?;
        debug!(
            "Network stack provision with local ip: {}",
            network_stack.ipv4_addr()
        );
        network_stack
            .setup_public_nat(&self.fc_config.public_network_interface)
            .map_err(|e| {
                error!("Failed to setup public NAT: {}", e);
                Status::internal("Failed to setup public NAT")
            })?;

        let machine = Machine::new(&self.fc_config, machine_config, network_stack)
            .await
            .map_err(|e| {
                info!("Failed to boot machine: {}", e);
                Status::internal("Failed to boot machine")
            })?;
        let uuid = machine.uuid().to_string();
        info!("Provisioned node {} ", &uuid);
        machines.insert(uuid.clone(), machine);
        Ok(Response::new(ProvisionResponse { id: uuid }))
    }

    async fn deprovision(&self, request: Request<InstanceId>) -> Result<Response<Empty>, Status> {
        let request = request.into_inner();

        let machine;
        {
            let mut machines = self.machines.write().await;
            machine = machines.remove(&request.id);
        }

        if let Some(machine) = machine {
            info!("Deprovisioning node with id {}", &request.id);
            let network_stack = machine.shutdown().await;
            self.network.lock().await.reclaim(network_stack);
            Ok(Response::new(Empty {}))
        } else {
            warn!(
                "Requested deprovisioning of missing machine with id {}",
                &request.id
            );
            Err(Status::not_found("Machine not found"))
        }
    }

    type StreamLogsStream = Pin<Box<dyn Stream<Item = Result<LogMessage, Status>> + Send>>;

    async fn stream_logs(
        &self,
        request: Request<InstanceId>,
    ) -> Result<Response<Self::StreamLogsStream>, Status> {
        let request = request.into_inner();
        let machines = self.machines.read().await;
        let machine = match machines.get(&request.id) {
            Some(machine) => machine,
            None => {
                warn!("Requested logs for missing machine with id {}", &request.id);
                return Err(Status::not_found("Machine not found"));
            }
        };

        let (logs, mut log_rx) = machine.get_and_subscribe_to_logs().await;

        let (tx, rpc_rx) = mpsc::channel(128);
        tokio::spawn(async move {
            for log in logs {
                let log_message = LogMessage { message: log };
                if let Err(_) = tx.send(Ok(log_message)).await {
                    return;
                }
            }
            while let Some(log) = log_rx.recv().await {
                let log_message = LogMessage {
                    message: log.to_string(),
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

    async fn get_logs(&self, request: Request<InstanceId>) -> Result<Response<AllLogs>, Status> {
        let request = request.into_inner();
        let machines = self.machines.read().await;
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
                .into_iter()
                .map(|s| LogMessage { message: s })
                .collect(),
        }))
    }

    async fn drain(&self, _: Request<Empty>) -> Result<Response<Empty>, Status> {
        let mut machines = self.machines.write().await;
        warn!("Draining all machines on node");
        let mut network_manager = self.network.lock().await;
        for (id, machine) in machines.drain() {
            info!("Deprovisioning id {}", &id);
            network_manager.reclaim(machine.shutdown().await);
        }
        Ok(Response::new(Empty {}))
    }
}

pub async fn run_server(manager: NodeManager) -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let server = NodeManagerServiceServer::new(manager);

    info!("NodeManager server listening on {}", addr);

    tonic::transport::Server::builder()
        .add_service(server)
        .serve(addr)
        .await?;
    Ok(())
}
