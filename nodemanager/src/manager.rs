use log::info;
use proto::node::node_manager_server::NodeManager as NodeManagerService;
use proto::node::node_manager_server::NodeManagerServer as NodeManagerServiceServer;
use proto::node::ProvisionRequest;
use proto::node::ProvisionResponse;
use tonic::Request;
use tonic::Response;
use tonic::Status;

pub struct NodeManager {}

#[tonic::async_trait]
impl NodeManagerService for NodeManager {
    async fn provision(
        &self,
        request: Request<ProvisionRequest>,
    ) -> Result<Response<ProvisionResponse>, Status> {
        info!("Provisioning node: {:?}", request);
        Err(Status::unimplemented("Not implemented"))
    }
}

pub async fn run_server(manager: NodeManager) -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let server = NodeManagerServiceServer::new(manager);

    tonic::transport::Server::builder()
        .add_service(server)
        .serve(addr)
        .await?;
    Ok(())
}
