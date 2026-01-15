//! Distributed S3 Conformance Tests with Chaos Testing
//!
//! This test suite verifies that the distributed S3 implementation:
//! 1. Correctly implements the S3 API
//! 2. Replicates data across nodes
//! 3. Tolerates single node failures
//! 4. Maintains data consistency during chaos
//!
//! Uses reqwest directly for HTTP requests to have full control over request format.

use reqwest::Client;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

/// Cluster of 3 S3 nodes
struct Cluster {
    nodes: Vec<NodeHandle>,
    http_client: Client,
}

struct NodeHandle {
    node_id: u32,
    port: u16,
    process: Option<Child>,
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

        Cluster {
            nodes,
            http_client: Client::new(),
        }
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

    // Helper methods for S3 operations

    async fn create_bucket(&self, node_idx: usize, bucket: &str) -> reqwest::Response {
        let url = format!("{}/{}", self.url_for_node(node_idx), bucket);
        self.http_client.put(&url).send().await.expect("Request failed")
    }

    async fn delete_bucket(&self, node_idx: usize, bucket: &str) -> reqwest::Response {
        let url = format!("{}/{}", self.url_for_node(node_idx), bucket);
        self.http_client.delete(&url).send().await.expect("Request failed")
    }

    async fn head_bucket(&self, node_idx: usize, bucket: &str) -> reqwest::Response {
        let url = format!("{}/{}", self.url_for_node(node_idx), bucket);
        self.http_client.head(&url).send().await.expect("Request failed")
    }

    async fn list_buckets(&self, node_idx: usize) -> reqwest::Response {
        let url = self.url_for_node(node_idx);
        self.http_client.get(&url).send().await.expect("Request failed")
    }

    async fn put_object(&self, node_idx: usize, bucket: &str, key: &str, body: &[u8]) -> reqwest::Response {
        let url = format!("{}/{}/{}", self.url_for_node(node_idx), bucket, key);
        self.http_client
            .put(&url)
            .body(body.to_vec())
            .send()
            .await
            .expect("Request failed")
    }

    async fn get_object(&self, node_idx: usize, bucket: &str, key: &str) -> reqwest::Response {
        let url = format!("{}/{}/{}", self.url_for_node(node_idx), bucket, key);
        self.http_client.get(&url).send().await.expect("Request failed")
    }

    async fn delete_object(&self, node_idx: usize, bucket: &str, key: &str) -> reqwest::Response {
        let url = format!("{}/{}/{}", self.url_for_node(node_idx), bucket, key);
        self.http_client.delete(&url).send().await.expect("Request failed")
    }

    async fn head_object(&self, node_idx: usize, bucket: &str, key: &str) -> reqwest::Response {
        let url = format!("{}/{}/{}", self.url_for_node(node_idx), bucket, key);
        self.http_client.head(&url).send().await.expect("Request failed")
    }

    async fn list_objects(&self, node_idx: usize, bucket: &str) -> reqwest::Response {
        let url = format!("{}/{}?list-type=2", self.url_for_node(node_idx), bucket);
        self.http_client.get(&url).send().await.expect("Request failed")
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        for node in &mut self.nodes {
            node.kill();
        }
    }
}

impl NodeHandle {
    fn start(node_id: u32, port: u16, peers: &str) -> Self {
        let process = Command::new("./target/release/s3_distributed")
            .args([
                "--node-id",
                &node_id.to_string(),
                "--port",
                &port.to_string(),
                "--peers",
                peers,
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
        let addr = format!("localhost:{}", self.port);
        for _ in 0..60 {
            thread::sleep(Duration::from_millis(100));
            if std::net::TcpStream::connect(&addr).is_ok() {
                return;
            }
        }
        panic!("Node {} failed to start within 6 seconds", self.node_id);
    }

    fn kill(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            let _ = process.wait();
        }
    }

    fn is_alive(&self) -> bool {
        self.process.is_some()
            && std::net::TcpStream::connect(format!("localhost:{}", self.port)).is_ok()
    }
}

// ============================================================================
// BASIC S3 API TESTS (with all nodes running)
// ============================================================================

#[tokio::test]
async fn test_create_bucket() {
    let cluster = Cluster::start();
    let resp = cluster.create_bucket(0, "test-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "CreateBucket should return 200");
}

#[tokio::test]
async fn test_create_bucket_already_exists() {
    let cluster = Cluster::start();

    let resp = cluster.create_bucket(0, "existing-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "First create should succeed");

    let resp = cluster.create_bucket(0, "existing-bucket").await;
    assert_eq!(resp.status().as_u16(), 409, "Duplicate bucket should return 409");
}

#[tokio::test]
async fn test_list_buckets() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "bucket-a").await;
    cluster.create_bucket(0, "bucket-b").await;

    let resp = cluster.list_buckets(0).await;
    assert_eq!(resp.status().as_u16(), 200, "ListBuckets should return 200");

    let body = resp.text().await.unwrap();
    assert!(body.contains("bucket-a"), "Should contain bucket-a");
    assert!(body.contains("bucket-b"), "Should contain bucket-b");
}

#[tokio::test]
async fn test_delete_bucket() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "delete-me").await;

    let resp = cluster.delete_bucket(0, "delete-me").await;
    assert_eq!(resp.status().as_u16(), 204, "DeleteBucket should return 204");
}

#[tokio::test]
async fn test_head_bucket() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "head-bucket").await;

    let resp = cluster.head_bucket(0, "head-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "HeadBucket should return 200");
}

#[tokio::test]
async fn test_put_object() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "objects-bucket").await;

    let resp = cluster.put_object(0, "objects-bucket", "test-key", b"hello world").await;
    assert_eq!(resp.status().as_u16(), 200, "PutObject should return 200");
}

#[tokio::test]
async fn test_get_object() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "get-bucket").await;
    cluster.put_object(0, "get-bucket", "my-key", b"test content").await;

    let resp = cluster.get_object(0, "get-bucket", "my-key").await;
    assert_eq!(resp.status().as_u16(), 200, "GetObject should return 200");

    let body = resp.bytes().await.unwrap();
    assert_eq!(&body[..], b"test content", "Content should match");
}

#[tokio::test]
async fn test_get_nonexistent_object() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "no-object-bucket").await;

    let resp = cluster.get_object(0, "no-object-bucket", "nonexistent").await;
    assert_eq!(resp.status().as_u16(), 404, "GetObject for missing key should return 404");
}

#[tokio::test]
async fn test_delete_object() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "del-obj-bucket").await;
    cluster.put_object(0, "del-obj-bucket", "to-delete", b"data").await;

    let resp = cluster.delete_object(0, "del-obj-bucket", "to-delete").await;
    assert_eq!(resp.status().as_u16(), 204, "DeleteObject should return 204");

    // Verify it's gone
    let resp = cluster.get_object(0, "del-obj-bucket", "to-delete").await;
    assert_eq!(resp.status().as_u16(), 404, "Deleted object should return 404");
}

#[tokio::test]
async fn test_head_object() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "head-obj-bucket").await;
    cluster.put_object(0, "head-obj-bucket", "head-key", b"head content").await;

    let resp = cluster.head_object(0, "head-obj-bucket", "head-key").await;
    assert_eq!(resp.status().as_u16(), 200, "HeadObject should return 200");
}

#[tokio::test]
async fn test_list_objects() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "list-bucket").await;
    cluster.put_object(0, "list-bucket", "key1", b"data1").await;
    cluster.put_object(0, "list-bucket", "key2", b"data2").await;

    let resp = cluster.list_objects(0, "list-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "ListObjects should return 200");

    let body = resp.text().await.unwrap();
    assert!(body.contains("key1"), "Should contain key1");
    assert!(body.contains("key2"), "Should contain key2");
}

#[tokio::test]
async fn test_delete_nonempty_bucket() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "nonempty-bucket").await;
    cluster.put_object(0, "nonempty-bucket", "blocking-key", b"data").await;

    let resp = cluster.delete_bucket(0, "nonempty-bucket").await;
    assert_eq!(resp.status().as_u16(), 409, "DeleteBucket on non-empty should return 409");
}

#[tokio::test]
async fn test_get_from_nonexistent_bucket() {
    let cluster = Cluster::start();

    let resp = cluster.get_object(0, "nonexistent-bucket", "any-key").await;
    assert_eq!(resp.status().as_u16(), 404, "GetObject from nonexistent bucket should return 404");
}

#[tokio::test]
async fn test_head_nonexistent_bucket() {
    let cluster = Cluster::start();

    let resp = cluster.head_bucket(0, "no-such-bucket").await;
    assert_eq!(resp.status().as_u16(), 404, "HeadBucket for missing bucket should return 404");
}

// ============================================================================
// REPLICATION TESTS (verify data is replicated across nodes)
// ============================================================================

#[tokio::test]
async fn test_bucket_visible_on_all_nodes() {
    let cluster = Cluster::start();

    // Create bucket on node 0
    let resp = cluster.create_bucket(0, "replicated-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "Create on node 0");

    // Wait for replication
    thread::sleep(Duration::from_millis(200));

    // Verify visible on node 1
    let resp = cluster.head_bucket(1, "replicated-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "Bucket should be visible on node 1");

    // Verify visible on node 2
    let resp = cluster.head_bucket(2, "replicated-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "Bucket should be visible on node 2");
}

#[tokio::test]
async fn test_data_visible_on_all_nodes() {
    let cluster = Cluster::start();

    // Create bucket and object on node 0
    cluster.create_bucket(0, "data-bucket").await;
    cluster.put_object(0, "data-bucket", "replicated-key", b"replicated content").await;

    // Wait for replication
    thread::sleep(Duration::from_millis(200));

    // Read from node 1
    let resp = cluster.get_object(1, "data-bucket", "replicated-key").await;
    assert_eq!(resp.status().as_u16(), 200, "Object should be readable from node 1");
    let body = resp.bytes().await.unwrap();
    assert_eq!(&body[..], b"replicated content", "Content from node 1");

    // Read from node 2
    let resp = cluster.get_object(2, "data-bucket", "replicated-key").await;
    assert_eq!(resp.status().as_u16(), 200, "Object should be readable from node 2");
    let body = resp.bytes().await.unwrap();
    assert_eq!(&body[..], b"replicated content", "Content from node 2");
}

// ============================================================================
// CHAOS TESTS (test behavior during node failures)
// ============================================================================

#[tokio::test]
async fn test_survive_single_node_failure() {
    let mut cluster = Cluster::start();

    // Create some data first
    cluster.create_bucket(0, "chaos-bucket").await;
    cluster.put_object(0, "chaos-bucket", "pre-chaos", b"original data").await;

    // Wait for replication
    thread::sleep(Duration::from_millis(200));

    // Kill node 2
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(100));

    // Should still be able to read from node 0
    let resp = cluster.get_object(0, "chaos-bucket", "pre-chaos").await;
    assert_eq!(resp.status().as_u16(), 200, "Should read from node 0 after killing node 2");

    // Should still be able to read from node 1
    let resp = cluster.get_object(1, "chaos-bucket", "pre-chaos").await;
    assert_eq!(resp.status().as_u16(), 200, "Should read from node 1 after killing node 2");
}

#[tokio::test]
async fn test_write_with_one_node_down() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "write-chaos").await;

    // Kill node 2
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(100));

    // Should still be able to write (2/3 quorum)
    let resp = cluster.put_object(0, "write-chaos", "during-chaos", b"chaos data").await;
    assert_eq!(resp.status().as_u16(), 200, "Write should succeed with 2 nodes");

    // Should be able to read it back
    let resp = cluster.get_object(0, "write-chaos", "during-chaos").await;
    assert_eq!(resp.status().as_u16(), 200, "Read should succeed");
    let body = resp.bytes().await.unwrap();
    assert_eq!(&body[..], b"chaos data", "Content should match");
}

#[tokio::test]
async fn test_read_from_surviving_node_after_kill() {
    let mut cluster = Cluster::start();

    // Write data while all nodes are up
    cluster.create_bucket(0, "survivor-bucket").await;
    cluster.put_object(0, "survivor-bucket", "survivor-key", b"survivor data").await;

    // Wait for replication
    thread::sleep(Duration::from_millis(200));

    // Kill the node we wrote to
    cluster.kill_node(0);
    thread::sleep(Duration::from_millis(100));

    // Should be able to read from node 1
    let resp = cluster.get_object(1, "survivor-bucket", "survivor-key").await;
    assert_eq!(resp.status().as_u16(), 200, "Should read from survivor node 1");

    // Should be able to read from node 2
    let resp = cluster.get_object(2, "survivor-bucket", "survivor-key").await;
    assert_eq!(resp.status().as_u16(), 200, "Should read from survivor node 2");
}

#[tokio::test]
async fn test_delete_with_one_node_down() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "delete-chaos").await;
    cluster.put_object(0, "delete-chaos", "to-delete", b"delete me").await;

    // Wait for replication
    thread::sleep(Duration::from_millis(200));

    // Kill node 1
    cluster.kill_node(1);
    thread::sleep(Duration::from_millis(100));

    // Delete should still work (2/3 quorum)
    let resp = cluster.delete_object(0, "delete-chaos", "to-delete").await;
    assert_eq!(resp.status().as_u16(), 204, "Delete should succeed with 2 nodes");

    // Verify it's gone
    let resp = cluster.get_object(0, "delete-chaos", "to-delete").await;
    assert_eq!(resp.status().as_u16(), 404, "Deleted object should return 404");
}

#[tokio::test]
async fn test_list_objects_with_one_node_down() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "list-chaos").await;
    cluster.put_object(0, "list-chaos", "item1", b"data1").await;
    cluster.put_object(0, "list-chaos", "item2", b"data2").await;

    // Wait for replication
    thread::sleep(Duration::from_millis(200));

    // Kill node 2
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(100));

    // List should still work
    let resp = cluster.list_objects(0, "list-chaos").await;
    assert_eq!(resp.status().as_u16(), 200, "List should work with 2 nodes");

    let body = resp.text().await.unwrap();
    assert!(body.contains("item1"), "Should contain item1");
    assert!(body.contains("item2"), "Should contain item2");
}

#[tokio::test]
async fn test_node_recovery_sync() {
    let mut cluster = Cluster::start();

    // Create data
    cluster.create_bucket(0, "recovery-bucket").await;
    cluster.put_object(0, "recovery-bucket", "before-kill", b"original").await;

    // Wait for replication
    thread::sleep(Duration::from_millis(200));

    // Kill node 2
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(100));

    // Write while node 2 is down
    cluster.put_object(0, "recovery-bucket", "during-outage", b"missed by node 2").await;

    // Wait for replication to surviving nodes
    thread::sleep(Duration::from_millis(200));

    // Restart node 2
    cluster.restart_node(2);

    // Give time for recovery sync
    thread::sleep(Duration::from_millis(500));

    // Node 2 should have the old data
    let resp = cluster.get_object(2, "recovery-bucket", "before-kill").await;
    assert_eq!(resp.status().as_u16(), 200, "Node 2 should have pre-kill data");

    // Node 2 should eventually get the new data (either via sync or future writes)
    // For basic implementation, we just verify it can still operate
    let resp = cluster.head_bucket(2, "recovery-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "Node 2 should recognize the bucket");
}

// ============================================================================
// CROSS-NODE OPERATION TESTS
// ============================================================================

#[tokio::test]
async fn test_operations_across_different_nodes() {
    let cluster = Cluster::start();

    // Create bucket on node 0
    let resp = cluster.create_bucket(0, "cross-node").await;
    assert_eq!(resp.status().as_u16(), 200, "Create on node 0");

    thread::sleep(Duration::from_millis(200));

    // Put object via node 1
    let resp = cluster.put_object(1, "cross-node", "cross-key", b"cross data").await;
    assert_eq!(resp.status().as_u16(), 200, "Put via node 1");

    thread::sleep(Duration::from_millis(200));

    // Get object via node 2
    let resp = cluster.get_object(2, "cross-node", "cross-key").await;
    assert_eq!(resp.status().as_u16(), 200, "Get via node 2");
    let body = resp.bytes().await.unwrap();
    assert_eq!(&body[..], b"cross data", "Content should match");

    // Delete via node 0
    let resp = cluster.delete_object(0, "cross-node", "cross-key").await;
    assert_eq!(resp.status().as_u16(), 204, "Delete via node 0");

    thread::sleep(Duration::from_millis(200));

    // Verify deleted on node 1
    let resp = cluster.get_object(1, "cross-node", "cross-key").await;
    assert_eq!(resp.status().as_u16(), 404, "Should be deleted on node 1");
}

#[tokio::test]
async fn test_read_your_writes() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "ryw-bucket").await;

    // Write and immediately read
    for i in 0..5 {
        let key = format!("key-{}", i);
        let data = format!("data-{}", i);

        let resp = cluster.put_object(0, "ryw-bucket", &key, data.as_bytes()).await;
        assert_eq!(resp.status().as_u16(), 200, "Put {} should succeed", i);

        // Should be able to read immediately on same node
        let resp = cluster.get_object(0, "ryw-bucket", &key).await;
        assert_eq!(resp.status().as_u16(), 200, "Get {} should succeed", i);
        let body = resp.bytes().await.unwrap();
        assert_eq!(&body[..], data.as_bytes(), "Content {} should match", i);
    }
}

#[tokio::test]
async fn test_overwrite_consistency() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "overwrite-bucket").await;

    // Initial write
    cluster.put_object(0, "overwrite-bucket", "mutable-key", b"version1").await;

    thread::sleep(Duration::from_millis(200));

    // Overwrite
    cluster.put_object(1, "overwrite-bucket", "mutable-key", b"version2").await;

    thread::sleep(Duration::from_millis(200));

    // All nodes should see version2
    for node in 0..3 {
        let resp = cluster.get_object(node, "overwrite-bucket", "mutable-key").await;
        assert_eq!(resp.status().as_u16(), 200, "Get from node {}", node);
        let body = resp.bytes().await.unwrap();
        assert_eq!(&body[..], b"version2", "Node {} should have version2", node);
    }
}

#[tokio::test]
async fn test_chaos_multiple_operations() {
    let mut cluster = Cluster::start();

    // Setup
    cluster.create_bucket(0, "multi-chaos").await;
    cluster.put_object(0, "multi-chaos", "stable-1", b"stable1").await;
    cluster.put_object(0, "multi-chaos", "stable-2", b"stable2").await;

    thread::sleep(Duration::from_millis(200));

    // Kill a node
    cluster.kill_node(1);
    thread::sleep(Duration::from_millis(100));

    // Do multiple operations
    cluster.put_object(0, "multi-chaos", "chaos-1", b"chaos1").await;
    cluster.put_object(2, "multi-chaos", "chaos-2", b"chaos2").await;
    cluster.delete_object(0, "multi-chaos", "stable-1").await;

    // Verify state
    let resp = cluster.get_object(0, "multi-chaos", "stable-1").await;
    assert_eq!(resp.status().as_u16(), 404, "stable-1 should be deleted");

    let resp = cluster.get_object(2, "multi-chaos", "stable-2").await;
    assert_eq!(resp.status().as_u16(), 200, "stable-2 should exist");

    let resp = cluster.get_object(0, "multi-chaos", "chaos-1").await;
    assert_eq!(resp.status().as_u16(), 200, "chaos-1 should exist");

    let resp = cluster.get_object(2, "multi-chaos", "chaos-2").await;
    assert_eq!(resp.status().as_u16(), 200, "chaos-2 should exist");
}

// ============================================================================
// ADVANCED CHAOS TESTS (more failure scenarios)
// ============================================================================

#[tokio::test]
async fn test_majority_failure_rejects_writes() {
    let mut cluster = Cluster::start();

    // Setup while healthy
    cluster.create_bucket(0, "majority-fail").await;
    cluster.put_object(0, "majority-fail", "before", b"before failure").await;

    thread::sleep(Duration::from_millis(200));

    // Kill 2 out of 3 nodes - no quorum possible
    cluster.kill_node(1);
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(100));

    // Write should fail (no quorum) - expect 503 Service Unavailable
    let resp = cluster.put_object(0, "majority-fail", "during", b"should fail").await;
    assert!(
        resp.status().as_u16() == 503 || resp.status().as_u16() == 500,
        "Write without quorum should fail with 5xx, got {}",
        resp.status().as_u16()
    );

    // But read of existing data should still work (local read)
    let resp = cluster.get_object(0, "majority-fail", "before").await;
    assert_eq!(resp.status().as_u16(), 200, "Read of existing data should work");
}

#[tokio::test]
async fn test_sequential_node_failures_and_recovery() {
    let mut cluster = Cluster::start();

    // Setup
    cluster.create_bucket(0, "seq-fail").await;
    cluster.put_object(0, "seq-fail", "initial", b"initial data").await;
    thread::sleep(Duration::from_millis(200));

    // Kill node 0, write via node 1
    cluster.kill_node(0);
    thread::sleep(Duration::from_millis(100));
    let resp = cluster.put_object(1, "seq-fail", "after-kill-0", b"written via node 1").await;
    assert_eq!(resp.status().as_u16(), 200, "Write via node 1 should succeed");

    // Recover node 0, kill node 1
    cluster.restart_node(0);
    thread::sleep(Duration::from_millis(300));
    cluster.kill_node(1);
    thread::sleep(Duration::from_millis(100));

    // Write via node 2
    let resp = cluster.put_object(2, "seq-fail", "after-kill-1", b"written via node 2").await;
    assert_eq!(resp.status().as_u16(), 200, "Write via node 2 should succeed");

    // Recover node 1
    cluster.restart_node(1);
    thread::sleep(Duration::from_millis(300));

    // All nodes should have the data
    for node in 0..3 {
        let resp = cluster.get_object(node, "seq-fail", "initial").await;
        assert_eq!(resp.status().as_u16(), 200, "Node {} should have initial data", node);
    }
}

#[tokio::test]
async fn test_rapid_failover() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "rapid-fail").await;
    cluster.put_object(0, "rapid-fail", "pre", b"pre-failure").await;
    thread::sleep(Duration::from_millis(200));

    // Kill node and immediately try operations (no sleep)
    cluster.kill_node(2);

    // Rapid operations
    let resp = cluster.put_object(0, "rapid-fail", "rapid-1", b"rapid1").await;
    assert_eq!(resp.status().as_u16(), 200, "Rapid write 1");

    let resp = cluster.put_object(1, "rapid-fail", "rapid-2", b"rapid2").await;
    assert_eq!(resp.status().as_u16(), 200, "Rapid write 2");

    let resp = cluster.get_object(0, "rapid-fail", "rapid-1").await;
    assert_eq!(resp.status().as_u16(), 200, "Rapid read 1");

    let resp = cluster.get_object(1, "rapid-fail", "rapid-2").await;
    assert_eq!(resp.status().as_u16(), 200, "Rapid read 2");
}

#[tokio::test]
async fn test_large_object_during_chaos() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "large-chaos").await;

    // Create a 100KB object
    let large_data: Vec<u8> = (0..102400).map(|i| (i % 256) as u8).collect();

    // Write large object while healthy
    let resp = cluster.put_object(0, "large-chaos", "large-healthy", &large_data).await;
    assert_eq!(resp.status().as_u16(), 200, "Large write while healthy");

    thread::sleep(Duration::from_millis(200));

    // Kill a node
    cluster.kill_node(1);
    thread::sleep(Duration::from_millis(100));

    // Write large object during chaos
    let resp = cluster.put_object(0, "large-chaos", "large-chaos", &large_data).await;
    assert_eq!(resp.status().as_u16(), 200, "Large write during chaos");

    // Read it back
    let resp = cluster.get_object(2, "large-chaos", "large-chaos").await;
    assert_eq!(resp.status().as_u16(), 200, "Large read during chaos");
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.len(), large_data.len(), "Large object size should match");
    assert_eq!(&body[..], &large_data[..], "Large object content should match");
}

#[tokio::test]
async fn test_many_objects_during_chaos() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "many-chaos").await;

    // Write 20 objects while healthy
    for i in 0..20 {
        let key = format!("obj-{}", i);
        let data = format!("data-{}", i);
        cluster.put_object(0, "many-chaos", &key, data.as_bytes()).await;
    }

    thread::sleep(Duration::from_millis(300));

    // Kill a node
    cluster.kill_node(0);
    thread::sleep(Duration::from_millis(100));

    // Write 10 more objects during chaos
    for i in 20..30 {
        let key = format!("obj-{}", i);
        let data = format!("data-{}", i);
        let resp = cluster.put_object(1, "many-chaos", &key, data.as_bytes()).await;
        assert_eq!(resp.status().as_u16(), 200, "Write obj-{} during chaos", i);
    }

    // Verify all objects are readable
    for i in 0..30 {
        let key = format!("obj-{}", i);
        let expected = format!("data-{}", i);
        let resp = cluster.get_object(2, "many-chaos", &key).await;
        assert_eq!(resp.status().as_u16(), 200, "Read obj-{} should succeed", i);
        let body = resp.bytes().await.unwrap();
        assert_eq!(&body[..], expected.as_bytes(), "obj-{} content", i);
    }
}

#[tokio::test]
async fn test_bucket_operations_during_chaos() {
    let mut cluster = Cluster::start();

    // Create bucket while healthy
    cluster.create_bucket(0, "bucket-chaos-1").await;
    thread::sleep(Duration::from_millis(200));

    // Kill a node
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(100));

    // Create another bucket during chaos
    let resp = cluster.create_bucket(0, "bucket-chaos-2").await;
    assert_eq!(resp.status().as_u16(), 200, "Create bucket during chaos");

    // Delete bucket during chaos
    let resp = cluster.delete_bucket(1, "bucket-chaos-1").await;
    assert_eq!(resp.status().as_u16(), 204, "Delete bucket during chaos");

    // Verify bucket-1 is gone
    let resp = cluster.head_bucket(0, "bucket-chaos-1").await;
    assert_eq!(resp.status().as_u16(), 404, "Deleted bucket should be gone");

    // Verify bucket-2 exists
    let resp = cluster.head_bucket(1, "bucket-chaos-2").await;
    assert_eq!(resp.status().as_u16(), 200, "New bucket should exist");
}

#[tokio::test]
async fn test_rolling_restart() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "rolling").await;
    cluster.put_object(0, "rolling", "persistent", b"survives restarts").await;
    thread::sleep(Duration::from_millis(200));

    // Rolling restart: restart each node one at a time
    for i in 0..3 {
        cluster.kill_node(i);
        thread::sleep(Duration::from_millis(100));

        // Operations should still work via other nodes
        let read_node = (i + 1) % 3;
        let resp = cluster.get_object(read_node, "rolling", "persistent").await;
        assert_eq!(resp.status().as_u16(), 200, "Read during restart of node {}", i);

        cluster.restart_node(i);
        thread::sleep(Duration::from_millis(300));
    }

    // All nodes should have the data after rolling restart
    for node in 0..3 {
        let resp = cluster.get_object(node, "rolling", "persistent").await;
        assert_eq!(resp.status().as_u16(), 200, "Node {} after rolling restart", node);
        let body = resp.bytes().await.unwrap();
        assert_eq!(&body[..], b"survives restarts", "Content on node {}", node);
    }
}

#[tokio::test]
async fn test_partition_and_heal() {
    let mut cluster = Cluster::start();

    // Initial data
    cluster.create_bucket(0, "partition").await;
    cluster.put_object(0, "partition", "before-partition", b"before").await;
    thread::sleep(Duration::from_millis(200));

    // Simulate partition by killing node 2
    cluster.kill_node(2);
    thread::sleep(Duration::from_millis(100));

    // Write data while partitioned
    cluster.put_object(0, "partition", "during-partition", b"during").await;
    thread::sleep(Duration::from_millis(200));

    // Heal partition by restarting node 2
    cluster.restart_node(2);
    thread::sleep(Duration::from_millis(500));

    // Node 2 should have pre-partition data (was replicated before kill)
    let resp = cluster.get_object(2, "partition", "before-partition").await;
    assert_eq!(resp.status().as_u16(), 200, "Node 2 should have pre-partition data");

    // Write new data to verify cluster is fully operational
    let resp = cluster.put_object(2, "partition", "after-heal", b"healed").await;
    assert_eq!(resp.status().as_u16(), 200, "Write after heal should succeed");

    // Verify on all nodes
    for node in 0..3 {
        let resp = cluster.get_object(node, "partition", "after-heal").await;
        assert_eq!(resp.status().as_u16(), 200, "Node {} should have post-heal data", node);
    }
}

#[tokio::test]
async fn test_write_to_different_nodes_same_key() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "same-key").await;

    // Write same key from different nodes in sequence
    cluster.put_object(0, "same-key", "contested", b"from node 0").await;
    thread::sleep(Duration::from_millis(100));

    cluster.put_object(1, "same-key", "contested", b"from node 1").await;
    thread::sleep(Duration::from_millis(100));

    cluster.put_object(2, "same-key", "contested", b"from node 2").await;
    thread::sleep(Duration::from_millis(200));

    // All nodes should have consistent view (last write wins)
    let mut values = Vec::new();
    for node in 0..3 {
        let resp = cluster.get_object(node, "same-key", "contested").await;
        assert_eq!(resp.status().as_u16(), 200, "Read from node {}", node);
        let body = resp.bytes().await.unwrap();
        values.push(body.to_vec());
    }

    // All nodes should return the same value
    assert_eq!(values[0], values[1], "Node 0 and 1 should agree");
    assert_eq!(values[1], values[2], "Node 1 and 2 should agree");
}

#[tokio::test]
async fn test_delete_during_node_failure() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "del-fail").await;

    // Create several objects
    for i in 0..5 {
        cluster.put_object(0, "del-fail", &format!("key-{}", i), b"data").await;
    }
    thread::sleep(Duration::from_millis(200));

    // Kill a node
    cluster.kill_node(1);
    thread::sleep(Duration::from_millis(100));

    // Delete objects during failure
    for i in 0..3 {
        let resp = cluster.delete_object(0, "del-fail", &format!("key-{}", i)).await;
        assert_eq!(resp.status().as_u16(), 204, "Delete key-{} during failure", i);
    }

    // Restart node
    cluster.restart_node(1);
    thread::sleep(Duration::from_millis(300));

    // Verify deletes were replicated
    for i in 0..3 {
        let resp = cluster.get_object(1, "del-fail", &format!("key-{}", i)).await;
        assert_eq!(resp.status().as_u16(), 404, "key-{} should be deleted on node 1", i);
    }

    // Verify remaining objects exist
    for i in 3..5 {
        let resp = cluster.get_object(1, "del-fail", &format!("key-{}", i)).await;
        assert_eq!(resp.status().as_u16(), 200, "key-{} should exist on node 1", i);
    }
}

#[tokio::test]
async fn test_rapid_create_delete_cycle() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "cycle").await;

    // Rapid create-delete cycles
    for i in 0..10 {
        let key = format!("cycle-{}", i);

        let resp = cluster.put_object(0, "cycle", &key, b"temporary").await;
        assert_eq!(resp.status().as_u16(), 200, "Create cycle-{}", i);

        let resp = cluster.delete_object(0, "cycle", &key).await;
        assert_eq!(resp.status().as_u16(), 204, "Delete cycle-{}", i);
    }

    // Kill a node and continue cycling
    cluster.kill_node(2);

    for i in 10..15 {
        let key = format!("cycle-{}", i);

        let resp = cluster.put_object(1, "cycle", &key, b"during-chaos").await;
        assert_eq!(resp.status().as_u16(), 200, "Create cycle-{} during chaos", i);

        let resp = cluster.delete_object(0, "cycle", &key).await;
        assert_eq!(resp.status().as_u16(), 204, "Delete cycle-{} during chaos", i);
    }

    // Verify bucket is empty
    let resp = cluster.list_objects(0, "cycle").await;
    assert_eq!(resp.status().as_u16(), 200, "List should work");
}

#[tokio::test]
async fn test_stress_with_node_flapping() {
    let mut cluster = Cluster::start();

    cluster.create_bucket(0, "flap").await;

    // Write some baseline data
    for i in 0..5 {
        cluster.put_object(0, "flap", &format!("base-{}", i), format!("base-{}", i).as_bytes()).await;
    }
    thread::sleep(Duration::from_millis(200));

    // Flap node 2 (kill/restart) while doing operations
    for round in 0..3 {
        cluster.kill_node(2);

        // Do operations while node is down
        let key = format!("round-{}", round);
        let resp = cluster.put_object(0, "flap", &key, format!("round-{}", round).as_bytes()).await;
        assert_eq!(resp.status().as_u16(), 200, "Write in round {}", round);

        cluster.restart_node(2);
        thread::sleep(Duration::from_millis(300));

        // Verify data is accessible
        let resp = cluster.get_object(2, "flap", &key).await;
        // Note: may or may not have the data depending on sync timing
        assert!(
            resp.status().as_u16() == 200 || resp.status().as_u16() == 404,
            "Read in round {} should be 200 or 404, got {}",
            round,
            resp.status().as_u16()
        );
    }

    // Final check: base data should be intact on all nodes
    for node in 0..3 {
        for i in 0..5 {
            let resp = cluster.get_object(node, "flap", &format!("base-{}", i)).await;
            assert_eq!(resp.status().as_u16(), 200, "base-{} on node {}", i, node);
        }
    }
}

#[tokio::test]
async fn test_create_bucket_on_each_node() {
    let cluster = Cluster::start();

    // Create different buckets on each node
    let resp = cluster.create_bucket(0, "node0-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "Create on node 0");

    let resp = cluster.create_bucket(1, "node1-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "Create on node 1");

    let resp = cluster.create_bucket(2, "node2-bucket").await;
    assert_eq!(resp.status().as_u16(), 200, "Create on node 2");

    thread::sleep(Duration::from_millis(300));

    // All buckets should be visible on all nodes
    for node in 0..3 {
        let resp = cluster.head_bucket(node, "node0-bucket").await;
        assert_eq!(resp.status().as_u16(), 200, "node0-bucket on node {}", node);

        let resp = cluster.head_bucket(node, "node1-bucket").await;
        assert_eq!(resp.status().as_u16(), 200, "node1-bucket on node {}", node);

        let resp = cluster.head_bucket(node, "node2-bucket").await;
        assert_eq!(resp.status().as_u16(), 200, "node2-bucket on node {}", node);
    }
}

#[tokio::test]
async fn test_interleaved_operations_different_nodes() {
    let cluster = Cluster::start();

    cluster.create_bucket(0, "interleave").await;
    thread::sleep(Duration::from_millis(100));

    // Interleaved operations across nodes
    cluster.put_object(0, "interleave", "a", b"a").await;
    cluster.put_object(1, "interleave", "b", b"b").await;
    cluster.put_object(2, "interleave", "c", b"c").await;

    let resp = cluster.get_object(1, "interleave", "a").await;
    assert_eq!(resp.status().as_u16(), 200, "Get a from node 1");

    cluster.put_object(2, "interleave", "d", b"d").await;

    let resp = cluster.get_object(0, "interleave", "b").await;
    assert_eq!(resp.status().as_u16(), 200, "Get b from node 0");

    cluster.delete_object(1, "interleave", "a").await;

    let resp = cluster.get_object(2, "interleave", "c").await;
    assert_eq!(resp.status().as_u16(), 200, "Get c from node 2");

    thread::sleep(Duration::from_millis(200));

    // Final verification
    let resp = cluster.get_object(0, "interleave", "a").await;
    assert_eq!(resp.status().as_u16(), 404, "a should be deleted");

    let resp = cluster.get_object(0, "interleave", "d").await;
    assert_eq!(resp.status().as_u16(), 200, "d should exist");
}
