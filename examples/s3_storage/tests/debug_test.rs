use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, Client, Config};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

#[tokio::test]
async fn test_debug_buckets() {
    // Start nodes
    let mut nodes: Vec<Child> = vec![];

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

    thread::sleep(Duration::from_secs(2));

    // Create client
    let creds = Credentials::new("test", "test", None, None, "test");
    let config = Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url("http://localhost:3001")
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    let client = Client::from_conf(config);

    // List buckets before creating any
    println!("Listing buckets before creation...");
    let result = client.list_buckets().send().await.unwrap();
    println!("Found {} buckets:", result.buckets().len());
    for bucket in result.buckets() {
        println!("  - {}", bucket.name().unwrap_or("UNNAMED"));
    }

    // Create bucket
    println!("\nCreating 'test-bucket'...");
    client.create_bucket()
        .bucket("test-bucket")
        .send()
        .await
        .unwrap();

    // List buckets after creation
    println!("\nListing buckets after creation...");
    let result = client.list_buckets().send().await.unwrap();
    println!("Found {} buckets:", result.buckets().len());
    for bucket in result.buckets() {
        println!("  - {}", bucket.name().unwrap_or("UNNAMED"));
    }

    // Clean up
    for mut node in nodes {
        let _ = node.kill();
    }
}