//! Simple distributed test without AWS SDK
//!
//! Run with: cargo test --test distributed_simple_test -- --test-threads=1

use reqwest::Client;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

struct Cluster {
    nodes: Vec<NodeHandle>,
    client: Client,
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

    fn wait_ready(&self) {
        for _ in 0..30 {
            if std::net::TcpStream::connect(format!("localhost:{}", self.port)).is_ok() {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
        panic!("Node {} failed to start on port {}", self.node_id, self.port);
    }

    fn kill(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            let _ = process.wait();
            thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        // Nodes will be dropped automatically
    }
}

impl Cluster {
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

        Cluster {
            nodes,
            client: Client::new(),
        }
    }

    fn url_for_node(&self, node_idx: usize) -> String {
        format!("http://localhost:{}", self.nodes[node_idx].port)
    }

    fn kill_node(&mut self, node_idx: usize) {
        self.nodes[node_idx].kill();
    }

    async fn create_bucket(&self, node_idx: usize, bucket: &str) -> reqwest::Response {
        let url = format!("{}/{}", self.url_for_node(node_idx), bucket);
        self.client.put(&url).send().await.expect("Request failed")
    }

    async fn list_buckets(&self, node_idx: usize) -> reqwest::Response {
        let url = self.url_for_node(node_idx);
        self.client.get(&url).send().await.expect("Request failed")
    }

    async fn put_object(&self, node_idx: usize, bucket: &str, key: &str, data: &[u8]) -> reqwest::Response {
        let url = format!("{}/{}/{}", self.url_for_node(node_idx), bucket, key);
        self.client.put(&url)
            .body(data.to_vec())
            .send()
            .await
            .expect("Request failed")
    }

    async fn get_object(&self, node_idx: usize, bucket: &str, key: &str) -> reqwest::Response {
        let url = format!("{}/{}/{}", self.url_for_node(node_idx), bucket, key);
        self.client.get(&url).send().await.expect("Request failed")
    }
}

#[tokio::test]
async fn test_simple_distributed_operations() {
    let cluster = Cluster::start();

    // Test 1: Create bucket on node 1
    let resp = cluster.create_bucket(0, "test-bucket").await;
    assert_eq!(resp.status(), 200, "Create bucket should succeed");

    // Test 2: List buckets from all nodes
    for i in 0..3 {
        let resp = cluster.list_buckets(i).await;
        assert_eq!(resp.status(), 200, "List buckets should succeed on node {}", i + 1);
        let text = resp.text().await.unwrap();
        assert!(text.contains("<Name>test-bucket</Name>"),
                "Bucket should be visible on node {}", i + 1);
    }

    // Test 3: Put object on node 1
    let resp = cluster.put_object(0, "test-bucket", "test.txt", b"Hello, World!").await;
    assert_eq!(resp.status(), 200, "Put object should succeed");

    // Test 4: Get object from all nodes
    thread::sleep(Duration::from_millis(200)); // Allow replication
    for i in 0..3 {
        let resp = cluster.get_object(i, "test-bucket", "test.txt").await;
        assert_eq!(resp.status(), 200, "Get object should succeed on node {}", i + 1);
        let data = resp.bytes().await.unwrap();
        assert_eq!(&data[..], b"Hello, World!",
                   "Object content should match on node {}", i + 1);
    }
}

#[tokio::test]
async fn test_simple_fault_tolerance() {
    let mut cluster = Cluster::start();

    // Create initial data
    cluster.create_bucket(0, "fault-test").await;
    cluster.put_object(0, "fault-test", "data.txt", b"Initial data").await;

    // Kill node 3
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(500));

    // Should still be able to write with 2 nodes
    let resp = cluster.create_bucket(0, "two-nodes").await;
    assert_eq!(resp.status(), 200, "Create bucket should work with 2 nodes");

    let resp = cluster.put_object(0, "two-nodes", "new.txt", b"Two node data").await;
    assert_eq!(resp.status(), 200, "Put object should work with 2 nodes");

    // Verify data on remaining nodes
    for i in 0..2 {
        let resp = cluster.get_object(i, "two-nodes", "new.txt").await;
        assert_eq!(resp.status(), 200, "Get object should succeed on node {}", i + 1);
        let data = resp.bytes().await.unwrap();
        assert_eq!(&data[..], b"Two node data");
    }
}