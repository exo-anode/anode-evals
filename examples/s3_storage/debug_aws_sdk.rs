use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, Client, Config};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Start 3 nodes
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

    // Wait for nodes to start
    thread::sleep(Duration::from_secs(3));

    println!("Creating AWS SDK client...");
    let creds = Credentials::new("test", "test", None, None, "test");
    let config = Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url("http://localhost:3001")
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    let client = Client::from_conf(config);

    println!("Attempting to create bucket...");
    match client.create_bucket()
        .bucket("debug-test-bucket")
        .send()
        .await {
        Ok(_) => println!("SUCCESS: Bucket created"),
        Err(e) => {
            println!("ERROR: {:?}", e);

            // Try to get more details
            if let aws_sdk_s3::Error::ServiceError(service_error) = &e {
                println!("Service Error Details:");
                println!("  Status: {:?}", service_error.raw().status());
                println!("  Headers: {:?}", service_error.raw().headers());
                if let Ok(body) = String::from_utf8(service_error.raw().body().bytes().unwrap_or(&[]).to_vec()) {
                    println!("  Body: {}", body);
                }
            }
        }
    }

    // Clean up
    for mut node in nodes {
        let _ = node.kill();
    }
}