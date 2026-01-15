//! Distributed S3 Conformance Tests with Chaos Testing
//!
//! This test suite verifies that the distributed S3 implementation:
//! 1. Correctly implements the S3 API
//! 2. Replicates data across nodes
//! 3. Tolerates single node failures
//! 4. Maintains data consistency during chaos
//!
//! Run with: cargo test --test distributed_conformance -- --test-threads=1

use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, Client, Config};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

/// Cluster of 3 S3 nodes
struct Cluster {
    nodes: Vec<NodeHandle>,
}

struct NodeHandle {
    node_id: u32,
    port: u16,
    process: Option<Child>,
}

impl Drop for NodeHandle {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }
    }
}

impl NodeHandle {
    /// Start a new node
    fn start(node_id: u32, port: u16, peers: &str) -> Self {
        let process = Command::new("./target/release/s3_storage")
            .args([
                "--node-id", &node_id.to_string(),
                "--port", &port.to_string(),
                "--peers", peers,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("Failed to start node");

        NodeHandle {
            node_id,
            port,
            process: Some(process),
        }
    }

    /// Wait for the node to be ready
    fn wait_ready(&self) {
        for _ in 0..30 {
            if std::net::TcpStream::connect(format!("localhost:{}", self.port)).is_ok() {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
        panic!("Node {} failed to start on port {}", self.node_id, self.port);
    }

    /// Check if the node is alive
    fn is_alive(&self) -> bool {
        std::net::TcpStream::connect(format!("localhost:{}", self.port)).is_ok()
    }

    /// Kill the node
    fn kill(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            let _ = process.wait();
            // Give OS time to release the port
            thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        // Nodes will be dropped automatically, killing their processes
    }
}

impl Cluster {
    /// Start a new 3-node cluster
    fn start() -> Self {
        // Build the project first
        let build_status = Command::new("cargo")
            .args(["build", "--release"])
            .status()
            .expect("Failed to build project");
        assert!(build_status.success(), "Failed to build project");

        let nodes = vec![
            NodeHandle::start(1, 3001, "http://localhost:3002,http://localhost:3003"),
            NodeHandle::start(2, 3002, "http://localhost:3001,http://localhost:3003"),
            NodeHandle::start(3, 3003, "http://localhost:3001,http://localhost:3002"),
        ];

        // Wait for all nodes to be ready
        for node in &nodes {
            node.wait_ready();
        }

        // Give cluster a moment to establish connections
        thread::sleep(Duration::from_millis(500));

        Cluster { nodes }
    }

    /// Get the base URL for a specific node
    fn url_for_node(&self, node_idx: usize) -> String {
        format!("http://localhost:{}", self.nodes[node_idx].port)
    }

    /// Kill a specific node (0-indexed)
    fn kill_node(&mut self, node_idx: usize) {
        self.nodes[node_idx].kill();
    }

    /// Restart a killed node
    fn restart_node(&mut self, node_idx: usize) {
        let node = &mut self.nodes[node_idx];
        let peers = match node.node_id {
            1 => "http://localhost:3002,http://localhost:3003",
            2 => "http://localhost:3001,http://localhost:3003",
            3 => "http://localhost:3001,http://localhost:3002",
            _ => panic!("Invalid node_id"),
        };
        *node = NodeHandle::start(node.node_id, node.port, peers);
        node.wait_ready();
        // Give node time to rejoin cluster
        thread::sleep(Duration::from_millis(300));
    }

    /// Check if a node is alive
    #[allow(dead_code)]
    fn is_node_alive(&self, node_idx: usize) -> bool {
        self.nodes[node_idx].is_alive()
    }

    /// Create an S3 client for a specific node
    fn create_client(&self, node_idx: usize) -> Client {
        let creds = Credentials::new("test", "test", None, None, "test");

        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .endpoint_url(self.url_for_node(node_idx))
            .credentials_provider(creds)
            .force_path_style(true)
            .build();

        Client::from_conf(config)
    }
}

// ============================================================================
// BASIC S3 CONFORMANCE TESTS
// ============================================================================

#[tokio::test]
async fn test_basic_bucket_operations() {
    let cluster = Cluster::start();
    let client = cluster.create_client(0);

    // Create bucket
    let result = client.create_bucket()
        .bucket("test-bucket")
        .send()
        .await;
    assert!(result.is_ok(), "CreateBucket should succeed: {:?}", result.err());

    // List buckets
    let result = client.list_buckets().send().await;
    assert!(result.is_ok(), "ListBuckets should succeed: {:?}", result.err());
    let buckets = result.unwrap();
    assert_eq!(buckets.buckets().len(), 1);
    assert_eq!(buckets.buckets()[0].name().unwrap(), "test-bucket");

    // Head bucket
    let result = client.head_bucket()
        .bucket("test-bucket")
        .send()
        .await;
    assert!(result.is_ok(), "HeadBucket should succeed: {:?}", result.err());

    // Delete bucket
    let result = client.delete_bucket()
        .bucket("test-bucket")
        .send()
        .await;
    assert!(result.is_ok(), "DeleteBucket should succeed: {:?}", result.err());
}

#[tokio::test]
async fn test_basic_object_operations() {
    let cluster = Cluster::start();
    let client = cluster.create_client(0);

    // Create bucket first
    client.create_bucket()
        .bucket("object-test")
        .send()
        .await
        .expect("Failed to create bucket");

    // Put object
    let content = b"Hello, Distributed World!";
    let result = client.put_object()
        .bucket("object-test")
        .key("test.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from(content.to_vec()))
        .send()
        .await;
    assert!(result.is_ok(), "PutObject should succeed: {:?}", result.err());

    // Get object
    let result = client.get_object()
        .bucket("object-test")
        .key("test.txt")
        .send()
        .await;
    assert!(result.is_ok(), "GetObject should succeed: {:?}", result.err());

    let data = result.unwrap().body.collect().await.unwrap().into_bytes();
    assert_eq!(data.as_ref(), content);

    // Head object
    let result = client.head_object()
        .bucket("object-test")
        .key("test.txt")
        .send()
        .await;
    assert!(result.is_ok(), "HeadObject should succeed: {:?}", result.err());
    assert_eq!(result.unwrap().content_length().unwrap(), content.len() as i64);

    // List objects
    let result = client.list_objects_v2()
        .bucket("object-test")
        .send()
        .await;
    assert!(result.is_ok(), "ListObjectsV2 should succeed: {:?}", result.err());
    let objects = result.unwrap();
    assert_eq!(objects.contents().len(), 1);
    assert_eq!(objects.contents()[0].key().unwrap(), "test.txt");

    // Delete object
    let result = client.delete_object()
        .bucket("object-test")
        .key("test.txt")
        .send()
        .await;
    assert!(result.is_ok(), "DeleteObject should succeed: {:?}", result.err());
}

// ============================================================================
// DISTRIBUTED REPLICATION TESTS
// ============================================================================

#[tokio::test]
async fn test_data_replication_across_nodes() {
    let cluster = Cluster::start();

    // Write to node 1
    let client1 = cluster.create_client(0);
    client1.create_bucket()
        .bucket("replicated-bucket")
        .send()
        .await
        .expect("Failed to create bucket");

    client1.put_object()
        .bucket("replicated-bucket")
        .key("replicated.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from(b"Replicated Data".to_vec()))
        .send()
        .await
        .expect("Failed to put object");

    // Give time for replication
    thread::sleep(Duration::from_millis(100));

    // Read from all nodes
    for i in 0..3 {
        let client = cluster.create_client(i);

        // Check bucket exists
        let buckets = client.list_buckets().send().await.expect("Failed to list buckets");
        assert!(buckets.buckets().iter().any(|b| b.name() == Some("replicated-bucket")),
                "Bucket should exist on node {}", i + 1);

        // Check object exists and has correct content
        let object = client.get_object()
            .bucket("replicated-bucket")
            .key("replicated.txt")
            .send()
            .await
            .expect(&format!("Failed to get object from node {}", i + 1));

        let data = object.body.collect().await.unwrap().into_bytes();
        assert_eq!(data.as_ref(), b"Replicated Data",
                   "Object content should match on node {}", i + 1);
    }
}

// ============================================================================
// FAULT TOLERANCE TESTS
// ============================================================================

#[tokio::test]
async fn test_single_node_failure_write_operations() {
    let mut cluster = Cluster::start();

    // Kill node 3
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(500));

    // Operations should still succeed with 2 nodes
    let client = cluster.create_client(0);

    // Create bucket with only 2 nodes
    let result = client.create_bucket()
        .bucket("two-node-bucket")
        .send()
        .await;
    assert!(result.is_ok(), "CreateBucket should succeed with 2 nodes: {:?}", result.err());

    // Put object with only 2 nodes
    let result = client.put_object()
        .bucket("two-node-bucket")
        .key("two-node.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from(b"Two node data".to_vec()))
        .send()
        .await;
    assert!(result.is_ok(), "PutObject should succeed with 2 nodes: {:?}", result.err());

    // Verify data is on both remaining nodes
    for i in 0..2 {
        let client = cluster.create_client(i);
        let object = client.get_object()
            .bucket("two-node-bucket")
            .key("two-node.txt")
            .send()
            .await
            .expect(&format!("Failed to get object from node {}", i + 1));

        let data = object.body.collect().await.unwrap().into_bytes();
        assert_eq!(data.as_ref(), b"Two node data");
    }
}

#[tokio::test]
async fn test_node_recovery_and_sync() {
    let mut cluster = Cluster::start();
    let client = cluster.create_client(0);

    // Create initial data
    client.create_bucket()
        .bucket("recovery-test")
        .send()
        .await
        .expect("Failed to create bucket");

    client.put_object()
        .bucket("recovery-test")
        .key("before-failure.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from(b"Before failure".to_vec()))
        .send()
        .await
        .expect("Failed to put object");

    // Kill node 3
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(500));

    // Write data while node 3 is down
    client.put_object()
        .bucket("recovery-test")
        .key("during-failure.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from(b"During failure".to_vec()))
        .send()
        .await
        .expect("Failed to put object during failure");

    // Restart node 3
    cluster.restart_node(2);
    thread::sleep(Duration::from_millis(1000)); // Give time for sync

    // Verify node 3 has all data
    let client3 = cluster.create_client(2);

    // Check bucket exists
    let buckets = client3.list_buckets().send().await.expect("Failed to list buckets");
    assert!(buckets.buckets().iter().any(|b| b.name() == Some("recovery-test")));

    // Check both objects exist
    let objects = client3.list_objects_v2()
        .bucket("recovery-test")
        .send()
        .await
        .expect("Failed to list objects");
    assert_eq!(objects.contents().len(), 2, "Should have both objects after recovery");

    // Verify content
    let before = client3.get_object()
        .bucket("recovery-test")
        .key("before-failure.txt")
        .send()
        .await
        .expect("Failed to get before-failure object");
    let before_data = before.body.collect().await.unwrap().into_bytes();
    assert_eq!(before_data.as_ref(), b"Before failure");

    let during = client3.get_object()
        .bucket("recovery-test")
        .key("during-failure.txt")
        .send()
        .await
        .expect("Failed to get during-failure object");
    let during_data = during.body.collect().await.unwrap().into_bytes();
    assert_eq!(during_data.as_ref(), b"During failure");
}

#[tokio::test]
async fn test_two_node_failure_rejection() {
    let mut cluster = Cluster::start();

    // Kill nodes 2 and 3
    cluster.kill_node(1);
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(500));

    // Operations should fail with only 1 node (can't achieve quorum)
    let client = cluster.create_client(0);

    // Try to create bucket - should fail
    let result = client.create_bucket()
        .bucket("single-node-bucket")
        .send()
        .await;
    assert!(result.is_err(), "CreateBucket should fail with only 1 node");

    // Try to put object in existing bucket - should also fail
    // First create a bucket while all nodes are up
    cluster.restart_node(1);
    cluster.restart_node(2);
    thread::sleep(Duration::from_millis(1000));

    client.create_bucket()
        .bucket("test-quorum")
        .send()
        .await
        .expect("Failed to create bucket");

    // Now kill 2 nodes again
    cluster.kill_node(1);
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(500));

    // Try to put object - should fail
    let result = client.put_object()
        .bucket("test-quorum")
        .key("should-fail.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from(b"Should fail".to_vec()))
        .send()
        .await;
    assert!(result.is_err(), "PutObject should fail with only 1 node");
}

// ============================================================================
// ERROR HANDLING TESTS
// ============================================================================

#[tokio::test]
async fn test_error_responses() {
    let cluster = Cluster::start();
    let client = cluster.create_client(0);

    // NoSuchBucket
    let result = client.get_object()
        .bucket("nonexistent")
        .key("test.txt")
        .send()
        .await;
    assert!(result.is_err());

    // BucketAlreadyExists
    client.create_bucket()
        .bucket("duplicate")
        .send()
        .await
        .expect("Failed to create bucket");

    let result = client.create_bucket()
        .bucket("duplicate")
        .send()
        .await;
    assert!(result.is_err());

    // BucketNotEmpty
    client.put_object()
        .bucket("duplicate")
        .key("file.txt")
        .body(aws_sdk_s3::primitives::ByteStream::from(b"data".to_vec()))
        .send()
        .await
        .expect("Failed to put object");

    let result = client.delete_bucket()
        .bucket("duplicate")
        .send()
        .await;
    assert!(result.is_err());

    // NoSuchKey
    client.create_bucket()
        .bucket("test-errors")
        .send()
        .await
        .expect("Failed to create bucket");

    let result = client.get_object()
        .bucket("test-errors")
        .key("nonexistent.txt")
        .send()
        .await;
    assert!(result.is_err());
}