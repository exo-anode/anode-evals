use std::process::Command;
use std::thread;
use std::time::Duration;

fn main() {
    println!("Starting 3 nodes...");

    let mut node1 = Command::new("./target/release/s3_storage")
        .args(&["--node-id", "1", "--port", "3001", "--peers", "http://localhost:3002,http://localhost:3003"])
        .spawn()
        .expect("Failed to start node 1");

    let mut node2 = Command::new("./target/release/s3_storage")
        .args(&["--node-id", "2", "--port", "3002", "--peers", "http://localhost:3001,http://localhost:3003"])
        .spawn()
        .expect("Failed to start node 2");

    let mut node3 = Command::new("./target/release/s3_storage")
        .args(&["--node-id", "3", "--port", "3003", "--peers", "http://localhost:3001,http://localhost:3002"])
        .spawn()
        .expect("Failed to start node 3");

    thread::sleep(Duration::from_secs(2));

    // Try to create a bucket with curl
    println!("\nTrying to create bucket with curl...");
    let output = Command::new("curl")
        .args(&["-X", "PUT", "http://localhost:3001/test-bucket", "-v"])
        .output()
        .expect("Failed to run curl");

    println!("Status: {:?}", output.status);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
    println!("Stderr: {}", String::from_utf8_lossy(&output.stderr));

    // Clean up
    let _ = node1.kill();
    let _ = node2.kill();
    let _ = node3.kill();
}