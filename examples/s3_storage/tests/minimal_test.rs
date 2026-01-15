use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, Client, Config};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

#[tokio::test]
async fn test_minimal() {
    println!("Starting nodes...");

    let mut nodes: Vec<Child> = vec![];

    // Start 3 nodes
    nodes.push(
        Command::new("./target/release/s3_storage")
            .args(["--node-id", "1", "--port", "3001", "--peers", "http://localhost:3002,http://localhost:3003"])
            .spawn()
            .expect("Failed to start node 1")
    );

    nodes.push(
        Command::new("./target/release/s3_storage")
            .args(["--node-id", "2", "--port", "3002", "--peers", "http://localhost:3001,http://localhost:3003"])
            .spawn()
            .expect("Failed to start node 2")
    );

    nodes.push(
        Command::new("./target/release/s3_storage")
            .args(["--node-id", "3", "--port", "3003", "--peers", "http://localhost:3001,http://localhost:3002"])
            .spawn()
            .expect("Failed to start node 3")
    );

    // Wait for nodes to start
    thread::sleep(Duration::from_secs(2));

    println!("Testing with curl first...");
    let output = Command::new("curl")
        .args(["-X", "PUT", "http://localhost:3001/test-bucket", "-i"])
        .output()
        .expect("Failed to run curl");

    println!("Curl output: {}", String::from_utf8_lossy(&output.stdout));

    // Now test with AWS SDK
    println!("Testing with AWS SDK...");
    let creds = Credentials::new("test", "test", None, None, "test");
    let config = Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url("http://localhost:3001")
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    let client = Client::from_conf(config);

    match client.create_bucket()
        .bucket("test-bucket-sdk")
        .send()
        .await {
        Ok(_) => println!("SDK: Success!"),
        Err(e) => println!("SDK: Error: {:?}", e),
    }

    // Clean up
    for mut node in nodes {
        let _ = node.kill();
    }
}