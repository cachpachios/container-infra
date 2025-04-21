use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use log::info;
use log::warn;
use proto::node::node_manager_server::NodeManager as NodeManagerService;
use proto::node::node_manager_server::NodeManagerServer as NodeManagerServiceServer;
use proto::node::Empty;
use proto::node::InstanceId;
use proto::node::ProvisionRequest;
use proto::node::ProvisionResponse;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tonic::Request;
use tonic::Response;
use tonic::Status;

use crate::machine;
use crate::machine::Machine;

pub struct NodeManager {
    machines: RwLock<HashMap<String, Arc<Mutex<Box<Machine>>>>>,
    fc_config: machine::FirecrackerConfig,
}

impl NodeManager {
    pub fn new(fc_config: machine::FirecrackerConfig) -> Self {
        NodeManager {
            machines: RwLock::new(HashMap::new()),
            fc_config,
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
        info!(
            "Provisioning node with container {}",
            &request.container_reference
        );

        let mut machines = self.machines.write().await;

        let machine_config = machine::MachineConfig {
            container_reference: request.container_reference,
        };

        let machine = Box::new(
            Machine::new(&self.fc_config, machine_config)
                .await
                .map_err(|e| {
                    info!("Failed to boot machine: {}", e);
                    Status::internal("Failed to boot machine")
                })?,
        );
        let uuid = machine.uuid().to_string();
        let machine = Arc::from(Mutex::from(machine));
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
            let mut machine = machine.lock().await;
            machine.shutdown().await;
            Ok(Response::new(Empty {}))
        } else {
            warn!(
                "Requested deprovisioning of missing machine with id {}",
                &request.id
            );
            Err(Status::not_found("Machine not found"))
        }
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
