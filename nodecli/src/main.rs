use proto::node::node_manager_client::NodeManagerClient;
use proto::node::ProvisionRequest;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = NodeManagerClient::connect("http://[::1]:50051").await?;

    let request = tonic::Request::new(ProvisionRequest {
        container_reference: "nginx:latest".into(),
    });

    let response = client.provision(request).await?;

    println!("RESPONSE={:?}", response);

    Ok(())
}
