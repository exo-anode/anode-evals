//! S3 Conformance Tests
//!
//! Tests the S3-compatible storage server using the official AWS SDK.
//! Run with: cargo test --test s3_conformance -- --test-threads=1

use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::{config::Region, Client, Config};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

const SERVER_URL: &str = "http://localhost:3000";

/// Guard that kills the server process when dropped
struct ServerGuard {
    process: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        // Give the OS time to release the port
        thread::sleep(Duration::from_millis(100));
    }
}

/// Start the S3 server and wait for it to be ready
fn start_server() -> ServerGuard {
    // Build the project first
    let build_status = Command::new("cargo")
        .args(["build", "--release"])
        .status()
        .expect("Failed to build project");

    assert!(build_status.success(), "Failed to build project");

    // Start the server binary directly (not via cargo run)
    // This ensures we can properly kill the process
    // Suppress server output to avoid interfering with test output
    let process = Command::new("./target/release/s3_storage")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to start server");

    // Wait for server to be ready
    for _ in 0..30 {
        thread::sleep(Duration::from_millis(200));
        // Simple TCP connection check instead of HTTP request
        if std::net::TcpStream::connect("localhost:3000").is_ok() {
            return ServerGuard { process };
        }
    }

    panic!("Server failed to start within 6 seconds");
}

/// Create an S3 client configured to use our local server
fn create_client() -> Client {
    let creds = Credentials::new("test", "test", None, None, "test");

    let config = Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(SERVER_URL)
        .credentials_provider(creds)
        .force_path_style(true)
        .build();

    Client::from_conf(config)
}

// ============================================================================
// BUCKET TESTS
// ============================================================================

#[tokio::test]
async fn test_create_bucket() {
    let _server = start_server();
    let client = create_client();

    let result = client.create_bucket()
        .bucket("test-bucket")
        .send()
        .await;

    assert!(result.is_ok(), "CreateBucket should succeed: {:?}", result.err());
}

#[tokio::test]
async fn test_create_bucket_already_exists() {
    let _server = start_server();
    let client = create_client();

    // Create bucket first time
    client.create_bucket()
        .bucket("duplicate-bucket")
        .send()
        .await
        .expect("First CreateBucket should succeed");

    // Try to create same bucket again
    let result = client.create_bucket()
        .bucket("duplicate-bucket")
        .send()
        .await;

    assert!(result.is_err(), "CreateBucket should fail for existing bucket");
}

#[tokio::test]
async fn test_head_bucket() {
    let _server = start_server();
    let client = create_client();

    // Create bucket
    client.create_bucket()
        .bucket("head-test-bucket")
        .send()
        .await
        .expect("CreateBucket should succeed");

    // Head bucket should succeed
    let result = client.head_bucket()
        .bucket("head-test-bucket")
        .send()
        .await;

    assert!(result.is_ok(), "HeadBucket should succeed for existing bucket: {:?}", result.err());
}

#[tokio::test]
async fn test_head_bucket_not_found() {
    let _server = start_server();
    let client = create_client();

    let result = client.head_bucket()
        .bucket("nonexistent-bucket")
        .send()
        .await;

    assert!(result.is_err(), "HeadBucket should fail for nonexistent bucket");
}

#[tokio::test]
async fn test_list_buckets_empty() {
    let _server = start_server();
    let client = create_client();

    let result = client.list_buckets()
        .send()
        .await
        .expect("ListBuckets should succeed");

    let buckets = result.buckets();
    assert!(buckets.is_empty(), "Should have no buckets initially");
}

#[tokio::test]
async fn test_list_buckets_with_buckets() {
    let _server = start_server();
    let client = create_client();

    // Create some buckets
    client.create_bucket().bucket("bucket-a").send().await.expect("Create bucket-a");
    client.create_bucket().bucket("bucket-b").send().await.expect("Create bucket-b");

    let result = client.list_buckets()
        .send()
        .await
        .expect("ListBuckets should succeed");

    let buckets = result.buckets();
    assert_eq!(buckets.len(), 2, "Should have 2 buckets");

    let names: Vec<&str> = buckets.iter()
        .filter_map(|b| b.name())
        .collect();
    assert!(names.contains(&"bucket-a"));
    assert!(names.contains(&"bucket-b"));
}

#[tokio::test]
async fn test_delete_bucket() {
    let _server = start_server();
    let client = create_client();

    // Create bucket
    client.create_bucket()
        .bucket("delete-me")
        .send()
        .await
        .expect("CreateBucket should succeed");

    // Delete bucket
    let result = client.delete_bucket()
        .bucket("delete-me")
        .send()
        .await;

    assert!(result.is_ok(), "DeleteBucket should succeed: {:?}", result.err());

    // Verify bucket is gone
    let head_result = client.head_bucket()
        .bucket("delete-me")
        .send()
        .await;

    assert!(head_result.is_err(), "Bucket should not exist after deletion");
}

#[tokio::test]
async fn test_delete_bucket_not_found() {
    let _server = start_server();
    let client = create_client();

    let result = client.delete_bucket()
        .bucket("nonexistent-bucket")
        .send()
        .await;

    assert!(result.is_err(), "DeleteBucket should fail for nonexistent bucket");
}

#[tokio::test]
async fn test_delete_bucket_not_empty() {
    let _server = start_server();
    let client = create_client();

    // Create bucket and add object
    client.create_bucket().bucket("not-empty").send().await.expect("Create bucket");
    client.put_object()
        .bucket("not-empty")
        .key("test.txt")
        .body(b"hello".to_vec().into())
        .send()
        .await
        .expect("PutObject should succeed");

    // Try to delete non-empty bucket
    let result = client.delete_bucket()
        .bucket("not-empty")
        .send()
        .await;

    assert!(result.is_err(), "DeleteBucket should fail for non-empty bucket");
}

// ============================================================================
// OBJECT TESTS
// ============================================================================

#[tokio::test]
async fn test_put_object() {
    let _server = start_server();
    let client = create_client();

    // Create bucket first
    client.create_bucket().bucket("objects").send().await.expect("Create bucket");

    let result = client.put_object()
        .bucket("objects")
        .key("test.txt")
        .body(b"Hello, World!".to_vec().into())
        .send()
        .await;

    assert!(result.is_ok(), "PutObject should succeed: {:?}", result.err());

    // Check that ETag is returned
    let response = result.unwrap();
    assert!(response.e_tag().is_some(), "ETag should be present");
}

#[tokio::test]
async fn test_get_object() {
    let _server = start_server();
    let client = create_client();

    // Setup
    client.create_bucket().bucket("get-test").send().await.expect("Create bucket");
    let content = b"Hello, S3!";
    client.put_object()
        .bucket("get-test")
        .key("greeting.txt")
        .body(content.to_vec().into())
        .send()
        .await
        .expect("PutObject should succeed");

    // Get the object
    let result = client.get_object()
        .bucket("get-test")
        .key("greeting.txt")
        .send()
        .await
        .expect("GetObject should succeed");

    // Verify content
    let body = result.body.collect().await.expect("Read body").into_bytes();
    assert_eq!(body.as_ref(), content, "Content should match");
}

#[tokio::test]
async fn test_get_object_not_found() {
    let _server = start_server();
    let client = create_client();

    // Create bucket but don't add object
    client.create_bucket().bucket("empty-bucket").send().await.expect("Create bucket");

    let result = client.get_object()
        .bucket("empty-bucket")
        .key("nonexistent.txt")
        .send()
        .await;

    assert!(result.is_err(), "GetObject should fail for nonexistent key");
}

#[tokio::test]
async fn test_head_object() {
    let _server = start_server();
    let client = create_client();

    // Setup
    client.create_bucket().bucket("head-obj").send().await.expect("Create bucket");
    let content = b"Test content for head";
    client.put_object()
        .bucket("head-obj")
        .key("file.txt")
        .body(content.to_vec().into())
        .send()
        .await
        .expect("PutObject should succeed");

    // Head the object
    let result = client.head_object()
        .bucket("head-obj")
        .key("file.txt")
        .send()
        .await
        .expect("HeadObject should succeed");

    // Verify metadata
    assert_eq!(result.content_length(), Some(content.len() as i64));
    assert!(result.e_tag().is_some(), "ETag should be present");
}

#[tokio::test]
async fn test_head_object_not_found() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("head-missing").send().await.expect("Create bucket");

    let result = client.head_object()
        .bucket("head-missing")
        .key("missing.txt")
        .send()
        .await;

    assert!(result.is_err(), "HeadObject should fail for nonexistent key");
}

#[tokio::test]
async fn test_delete_object() {
    let _server = start_server();
    let client = create_client();

    // Setup
    client.create_bucket().bucket("del-obj").send().await.expect("Create bucket");
    client.put_object()
        .bucket("del-obj")
        .key("delete-me.txt")
        .body(b"bye".to_vec().into())
        .send()
        .await
        .expect("PutObject should succeed");

    // Delete object
    let result = client.delete_object()
        .bucket("del-obj")
        .key("delete-me.txt")
        .send()
        .await;

    assert!(result.is_ok(), "DeleteObject should succeed: {:?}", result.err());

    // Verify object is gone
    let get_result = client.get_object()
        .bucket("del-obj")
        .key("delete-me.txt")
        .send()
        .await;

    assert!(get_result.is_err(), "Object should not exist after deletion");
}

#[tokio::test]
async fn test_list_objects_empty() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("list-empty").send().await.expect("Create bucket");

    let result = client.list_objects_v2()
        .bucket("list-empty")
        .send()
        .await
        .expect("ListObjectsV2 should succeed");

    let contents = result.contents();
    assert!(contents.is_empty(), "Should have no objects");
}

#[tokio::test]
async fn test_list_objects_with_objects() {
    let _server = start_server();
    let client = create_client();

    // Setup
    client.create_bucket().bucket("list-test").send().await.expect("Create bucket");
    client.put_object().bucket("list-test").key("a.txt").body(b"a".to_vec().into()).send().await.expect("Put a");
    client.put_object().bucket("list-test").key("b.txt").body(b"b".to_vec().into()).send().await.expect("Put b");
    client.put_object().bucket("list-test").key("c.txt").body(b"c".to_vec().into()).send().await.expect("Put c");

    let result = client.list_objects_v2()
        .bucket("list-test")
        .send()
        .await
        .expect("ListObjectsV2 should succeed");

    let contents = result.contents();
    assert_eq!(contents.len(), 3, "Should have 3 objects");

    let keys: Vec<&str> = contents.iter()
        .filter_map(|o| o.key())
        .collect();
    assert!(keys.contains(&"a.txt"));
    assert!(keys.contains(&"b.txt"));
    assert!(keys.contains(&"c.txt"));
}

#[tokio::test]
async fn test_list_objects_with_prefix() {
    let _server = start_server();
    let client = create_client();

    // Setup with different prefixes
    client.create_bucket().bucket("prefix-test").send().await.expect("Create bucket");
    client.put_object().bucket("prefix-test").key("docs/a.txt").body(b"a".to_vec().into()).send().await.expect("Put");
    client.put_object().bucket("prefix-test").key("docs/b.txt").body(b"b".to_vec().into()).send().await.expect("Put");
    client.put_object().bucket("prefix-test").key("images/c.png").body(b"c".to_vec().into()).send().await.expect("Put");

    // List with prefix
    let result = client.list_objects_v2()
        .bucket("prefix-test")
        .prefix("docs/")
        .send()
        .await
        .expect("ListObjectsV2 should succeed");

    let contents = result.contents();
    assert_eq!(contents.len(), 2, "Should have 2 objects with docs/ prefix");

    for obj in contents {
        assert!(obj.key().unwrap().starts_with("docs/"), "All keys should start with docs/");
    }
}

#[tokio::test]
async fn test_list_objects_pagination() {
    let _server = start_server();
    let client = create_client();

    // Setup with multiple objects
    client.create_bucket().bucket("page-test").send().await.expect("Create bucket");
    for i in 0..5 {
        client.put_object()
            .bucket("page-test")
            .key(format!("file{}.txt", i))
            .body(format!("content{}", i).into_bytes().into())
            .send()
            .await
            .expect("Put object");
    }

    // List with max_keys=2
    let result = client.list_objects_v2()
        .bucket("page-test")
        .max_keys(2)
        .send()
        .await
        .expect("ListObjectsV2 should succeed");

    let contents = result.contents();
    assert_eq!(contents.len(), 2, "Should return max_keys objects");
    assert_eq!(result.is_truncated(), Some(true), "Should be truncated");
    assert!(result.next_continuation_token().is_some(), "Should have continuation token");
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[tokio::test]
async fn test_full_workflow() {
    let _server = start_server();
    let client = create_client();

    // 1. Create bucket
    client.create_bucket().bucket("workflow").send().await.expect("Create bucket");

    // 2. Put objects
    client.put_object()
        .bucket("workflow")
        .key("data/file1.txt")
        .body(b"First file content".to_vec().into())
        .send()
        .await
        .expect("Put file1");

    client.put_object()
        .bucket("workflow")
        .key("data/file2.txt")
        .body(b"Second file content".to_vec().into())
        .send()
        .await
        .expect("Put file2");

    // 3. List objects
    let list_result = client.list_objects_v2()
        .bucket("workflow")
        .prefix("data/")
        .send()
        .await
        .expect("List objects");
    assert_eq!(list_result.contents().len(), 2);

    // 4. Get object
    let get_result = client.get_object()
        .bucket("workflow")
        .key("data/file1.txt")
        .send()
        .await
        .expect("Get file1");
    let body = get_result.body.collect().await.expect("Read body").into_bytes();
    assert_eq!(body.as_ref(), b"First file content");

    // 5. Delete objects
    client.delete_object().bucket("workflow").key("data/file1.txt").send().await.expect("Delete file1");
    client.delete_object().bucket("workflow").key("data/file2.txt").send().await.expect("Delete file2");

    // 6. Verify empty
    let final_list = client.list_objects_v2()
        .bucket("workflow")
        .send()
        .await
        .expect("Final list");
    assert!(final_list.contents().is_empty());

    // 7. Delete bucket
    client.delete_bucket().bucket("workflow").send().await.expect("Delete bucket");
}

#[tokio::test]
async fn test_large_object() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("large").send().await.expect("Create bucket");

    // Create ~1MB of data
    let large_content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

    // Put large object
    client.put_object()
        .bucket("large")
        .key("bigfile.bin")
        .body(large_content.clone().into())
        .send()
        .await
        .expect("Put large object");

    // Get and verify
    let result = client.get_object()
        .bucket("large")
        .key("bigfile.bin")
        .send()
        .await
        .expect("Get large object");

    let body = result.body.collect().await.expect("Read body").into_bytes();
    assert_eq!(body.len(), large_content.len(), "Size should match");
    assert_eq!(body.as_ref(), large_content.as_slice(), "Content should match");
}

// ============================================================================
// ADDITIONAL S3 API COVERAGE TESTS
// ============================================================================

#[tokio::test]
async fn test_object_overwrite() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("overwrite-test").send().await.expect("Create bucket");

    // Put initial object
    client.put_object()
        .bucket("overwrite-test")
        .key("file.txt")
        .body(b"original content".to_vec().into())
        .send()
        .await
        .expect("Put original");

    // Overwrite with new content
    client.put_object()
        .bucket("overwrite-test")
        .key("file.txt")
        .body(b"new content".to_vec().into())
        .send()
        .await
        .expect("Put overwrite");

    // Verify new content
    let result = client.get_object()
        .bucket("overwrite-test")
        .key("file.txt")
        .send()
        .await
        .expect("Get object");

    let body = result.body.collect().await.expect("Read body").into_bytes();
    assert_eq!(body.as_ref(), b"new content", "Content should be overwritten");
}

#[tokio::test]
async fn test_content_type() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("content-type-test").send().await.expect("Create bucket");

    // Put object with content type
    client.put_object()
        .bucket("content-type-test")
        .key("data.json")
        .content_type("application/json")
        .body(b"{\"key\": \"value\"}".to_vec().into())
        .send()
        .await
        .expect("Put with content type");

    // Get and verify content type
    let result = client.get_object()
        .bucket("content-type-test")
        .key("data.json")
        .send()
        .await
        .expect("Get object");

    assert_eq!(result.content_type(), Some("application/json"), "Content-Type should match");
}

#[tokio::test]
async fn test_delete_nonexistent_object() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("delete-noexist").send().await.expect("Create bucket");

    // Delete should succeed even if object doesn't exist (S3 idempotent delete)
    let result = client.delete_object()
        .bucket("delete-noexist")
        .key("does-not-exist.txt")
        .send()
        .await;

    assert!(result.is_ok(), "DeleteObject should succeed for nonexistent key: {:?}", result.err());
}

#[tokio::test]
async fn test_list_objects_continuation() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("continuation-test").send().await.expect("Create bucket");

    // Create 5 objects
    for i in 0..5 {
        client.put_object()
            .bucket("continuation-test")
            .key(format!("file{:02}.txt", i))
            .body(format!("content{}", i).into_bytes().into())
            .send()
            .await
            .expect("Put object");
    }

    // First page with max_keys=2
    let page1 = client.list_objects_v2()
        .bucket("continuation-test")
        .max_keys(2)
        .send()
        .await
        .expect("First page");

    assert_eq!(page1.contents().len(), 2, "First page should have 2 objects");
    assert_eq!(page1.is_truncated(), Some(true), "Should be truncated");
    let token = page1.next_continuation_token().expect("Should have token");

    // Second page using continuation token
    let page2 = client.list_objects_v2()
        .bucket("continuation-test")
        .max_keys(2)
        .continuation_token(token)
        .send()
        .await
        .expect("Second page");

    assert_eq!(page2.contents().len(), 2, "Second page should have 2 objects");
    assert_eq!(page2.is_truncated(), Some(true), "Should still be truncated");

    // Verify no duplicates between pages
    let page1_keys: Vec<&str> = page1.contents().iter().filter_map(|o| o.key()).collect();
    let page2_keys: Vec<&str> = page2.contents().iter().filter_map(|o| o.key()).collect();
    for key in &page1_keys {
        assert!(!page2_keys.contains(key), "Pages should not overlap");
    }
}

#[tokio::test]
async fn test_put_object_to_nonexistent_bucket() {
    let _server = start_server();
    let client = create_client();

    // Don't create bucket, try to put object
    let result = client.put_object()
        .bucket("nonexistent-bucket")
        .key("file.txt")
        .body(b"content".to_vec().into())
        .send()
        .await;

    assert!(result.is_err(), "PutObject should fail for nonexistent bucket");
}

#[tokio::test]
async fn test_get_object_from_nonexistent_bucket() {
    let _server = start_server();
    let client = create_client();

    // Don't create bucket, try to get object
    let result = client.get_object()
        .bucket("nonexistent-bucket")
        .key("file.txt")
        .send()
        .await;

    assert!(result.is_err(), "GetObject should fail for nonexistent bucket");
}

#[tokio::test]
async fn test_empty_object() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("empty-obj-test").send().await.expect("Create bucket");

    // Put empty object
    client.put_object()
        .bucket("empty-obj-test")
        .key("empty.txt")
        .body(Vec::new().into())
        .send()
        .await
        .expect("Put empty object");

    // Get and verify
    let result = client.get_object()
        .bucket("empty-obj-test")
        .key("empty.txt")
        .send()
        .await
        .expect("Get empty object");

    let content_length = result.content_length();
    let body = result.body.collect().await.expect("Read body").into_bytes();
    assert!(body.is_empty(), "Empty object should have no content");
    assert_eq!(content_length, Some(0), "Content-Length should be 0");
}

#[tokio::test]
async fn test_special_characters_in_key() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("special-chars").send().await.expect("Create bucket");

    // Keys with special characters
    let special_keys = vec![
        "file with spaces.txt",
        "path/to/nested/file.txt",
        "file-with-dashes.txt",
        "file_with_underscores.txt",
    ];

    for key in &special_keys {
        client.put_object()
            .bucket("special-chars")
            .key(*key)
            .body(format!("content for {}", key).into_bytes().into())
            .send()
            .await
            .expect(&format!("Put object with key: {}", key));

        // Verify we can get it back
        let result = client.get_object()
            .bucket("special-chars")
            .key(*key)
            .send()
            .await
            .expect(&format!("Get object with key: {}", key));

        let body = result.body.collect().await.expect("Read body").into_bytes();
        assert_eq!(
            body.as_ref(),
            format!("content for {}", key).as_bytes(),
            "Content should match for key: {}",
            key
        );
    }

    // List and verify all objects
    let list = client.list_objects_v2()
        .bucket("special-chars")
        .send()
        .await
        .expect("List objects");

    assert_eq!(list.contents().len(), special_keys.len(), "Should have all objects");
}

#[tokio::test]
async fn test_binary_content() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("binary-test").send().await.expect("Create bucket");

    // Create binary content with all byte values
    let binary_content: Vec<u8> = (0u8..=255).collect();

    client.put_object()
        .bucket("binary-test")
        .key("binary.bin")
        .body(binary_content.clone().into())
        .send()
        .await
        .expect("Put binary object");

    let result = client.get_object()
        .bucket("binary-test")
        .key("binary.bin")
        .send()
        .await
        .expect("Get binary object");

    let body = result.body.collect().await.expect("Read body").into_bytes();
    assert_eq!(body.as_ref(), binary_content.as_slice(), "Binary content should be preserved exactly");
}

#[tokio::test]
async fn test_list_objects_bucket_not_found() {
    let _server = start_server();
    let client = create_client();

    let result = client.list_objects_v2()
        .bucket("nonexistent-bucket")
        .send()
        .await;

    assert!(result.is_err(), "ListObjectsV2 should fail for nonexistent bucket");
}

#[tokio::test]
async fn test_head_object_metadata() {
    let _server = start_server();
    let client = create_client();

    client.create_bucket().bucket("head-meta").send().await.expect("Create bucket");

    let content = b"Test content for metadata verification";
    client.put_object()
        .bucket("head-meta")
        .key("meta.txt")
        .content_type("text/plain")
        .body(content.to_vec().into())
        .send()
        .await
        .expect("Put object");

    let result = client.head_object()
        .bucket("head-meta")
        .key("meta.txt")
        .send()
        .await
        .expect("Head object");

    // Verify all metadata
    assert_eq!(result.content_length(), Some(content.len() as i64), "Content-Length should match");
    assert!(result.e_tag().is_some(), "ETag should be present");
    assert!(result.last_modified().is_some(), "Last-Modified should be present");
}
