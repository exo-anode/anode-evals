use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, Client, Config};

#[tokio::main]
async fn main() {
    // Start 3 nodes manually first
    println!("Starting nodes...");
    let node1 = std::process::Command::new("./target/release/s3_storage")
        .args(["--node-id", "1", "--port", "3001", "--peers", "http://localhost:3002,http://localhost:3003"])
        .spawn()
        .expect("Failed to start node 1");

    let node2 = std::process::Command::new("./target/release/s3_storage")
        .args(["--node-id", "2", "--port", "3002", "--peers", "http://localhost:3001,http://localhost:3003"])
        .spawn()
        .expect("Failed to start node 2");

    let node3 = std::process::Command::new("./target/release/s3_storage")
        .args(["--node-id", "3", "--port", "3003", "--peers", "http://localhost:3001,http://localhost:3002"])
        .spawn()
        .expect("Failed to start node 3");

    // Wait for nodes to start
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Create S3 client
    let creds = Credentials::new("test", "test", None, None, "test");
    let config = Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url("http://localhost:3001")
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    let client = Client::from_conf(config);

    println!("Creating bucket...");
    match client.create_bucket()
        .bucket("test-debug-bucket")
        .send()
        .await {
        Ok(_) => println!("Success!"),
        Err(e) => println!("Error: {:?}", e),
    }

    // Clean up
    drop(node1);
    drop(node2);
    drop(node3);
}