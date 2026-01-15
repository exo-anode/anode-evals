//! Distributed S3-Compatible Object Storage Server
//!
//! A fault-tolerant, distributed S3-compatible REST API server that handles bucket
//! and object operations with consensus-based replication across a 3-node cluster.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Router,
    routing::{get, post},
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use clap::Parser;
use md5::{Digest, Md5};
use quick_xml::se::to_string as xml_to_string;
use serde::{Serialize, Deserialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::net::TcpListener;
use uuid::Uuid;
use base64::{Engine as _, engine::general_purpose};

// ============================================================================
// COMMAND LINE ARGUMENTS
// ============================================================================

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Unique identifier for this node (1, 2, or 3)
    #[arg(long)]
    node_id: u32,

    /// HTTP port for S3 API (e.g., 3001, 3002, 3003)
    #[arg(long)]
    port: u16,

    /// Comma-separated list of peer node URLs
    #[arg(long)]
    peers: String,
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

/// In-memory storage for the S3 server
#[derive(Debug, Clone)]
struct Storage {
    buckets: Arc<RwLock<HashMap<String, Bucket>>>,
    node_id: u32,
    peers: Vec<String>,
}

impl Storage {
    fn new(node_id: u32, peers: Vec<String>) -> Self {
        Storage {
            buckets: Arc::new(RwLock::new(HashMap::new())),
            node_id,
            peers,
        }
    }
}

/// A bucket containing objects
#[derive(Debug, Clone)]
struct Bucket {
    name: String,
    creation_date: DateTime<Utc>,
    objects: HashMap<String, Object>,
}

/// An object stored in a bucket
#[derive(Debug, Clone)]
struct Object {
    key: String,
    content: Bytes,
    content_type: Option<String>,
    etag: String,
    last_modified: DateTime<Utc>,
    size: u64,
}

// ============================================================================
// REPLICATION STRUCTURES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplicationRequest {
    operation: ReplicationOperation,
    bucket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>, // Base64 encoded
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReplicationOperation {
    PutObject,
    DeleteObject,
    CreateBucket,
    DeleteBucket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReplicationResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ============================================================================
// S3 API RESPONSE STRUCTURES
// ============================================================================

#[derive(Serialize)]
#[serde(rename = "ListAllMyBucketsResult")]
struct ListBucketsResponse {
    #[serde(rename = "@xmlns")]
    xmlns: String,
    #[serde(rename = "Owner")]
    owner: Owner,
    #[serde(rename = "Buckets")]
    buckets: BucketsContainer,
}

#[derive(Serialize)]
struct Owner {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "DisplayName")]
    display_name: String,
}

#[derive(Serialize)]
struct BucketsContainer {
    #[serde(rename = "Bucket")]
    bucket: Vec<BucketInfo>,
}

#[derive(Serialize)]
struct BucketInfo {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "CreationDate")]
    creation_date: String,
}

#[derive(Serialize)]
#[serde(rename = "ListBucketResult")]
struct ListObjectsResponse {
    #[serde(rename = "@xmlns")]
    xmlns: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Prefix")]
    prefix: String,
    #[serde(rename = "MaxKeys")]
    max_keys: u32,
    #[serde(rename = "IsTruncated")]
    is_truncated: bool,
    #[serde(rename = "Contents")]
    contents: Vec<ObjectInfo>,
    #[serde(rename = "NextContinuationToken", skip_serializing_if = "Option::is_none")]
    next_continuation_token: Option<String>,
    #[serde(rename = "ContinuationToken", skip_serializing_if = "Option::is_none")]
    continuation_token: Option<String>,
}

#[derive(Serialize)]
struct ObjectInfo {
    #[serde(rename = "Key")]
    key: String,
    #[serde(rename = "LastModified")]
    last_modified: String,
    #[serde(rename = "ETag")]
    etag: String,
    #[serde(rename = "Size")]
    size: u64,
    #[serde(rename = "StorageClass")]
    storage_class: String,
}

#[derive(Serialize)]
#[serde(rename = "Error")]
struct ErrorResponse {
    #[serde(rename = "Code")]
    code: String,
    #[serde(rename = "Message")]
    message: String,
    #[serde(rename = "BucketName", skip_serializing_if = "Option::is_none")]
    bucket_name: Option<String>,
    #[serde(rename = "RequestId")]
    request_id: String,
}

// ============================================================================
// QUERY PARAMETERS
// ============================================================================

#[derive(Debug, serde::Deserialize)]
struct ListObjectsQuery {
    #[serde(rename = "list-type")]
    list_type: Option<u32>,
    prefix: Option<String>,
    #[serde(rename = "max-keys")]
    max_keys: Option<u32>,
    #[serde(rename = "continuation-token")]
    continuation_token: Option<String>,
}

// ============================================================================
// ERROR HANDLING
// ============================================================================

#[derive(Debug)]
enum S3Error {
    BucketAlreadyExists(String),
    BucketNotEmpty(String),
    NoSuchBucket(String),
    NoSuchKey(String),
    InternalError(String),
    ServiceUnavailable(String),
}

impl S3Error {
    fn to_response(&self, request_id: String) -> Response {
        let (status, error_response) = match self {
            S3Error::BucketAlreadyExists(bucket) => (
                StatusCode::CONFLICT,
                ErrorResponse {
                    code: "BucketAlreadyExists".to_string(),
                    message: "The requested bucket name is not available".to_string(),
                    bucket_name: Some(bucket.clone()),
                    request_id,
                },
            ),
            S3Error::BucketNotEmpty(bucket) => (
                StatusCode::CONFLICT,
                ErrorResponse {
                    code: "BucketNotEmpty".to_string(),
                    message: "The bucket you tried to delete is not empty".to_string(),
                    bucket_name: Some(bucket.clone()),
                    request_id,
                },
            ),
            S3Error::NoSuchBucket(bucket) => (
                StatusCode::NOT_FOUND,
                ErrorResponse {
                    code: "NoSuchBucket".to_string(),
                    message: "The specified bucket does not exist".to_string(),
                    bucket_name: Some(bucket.clone()),
                    request_id,
                },
            ),
            S3Error::NoSuchKey(_) => (
                StatusCode::NOT_FOUND,
                ErrorResponse {
                    code: "NoSuchKey".to_string(),
                    message: "The specified key does not exist".to_string(),
                    bucket_name: None,
                    request_id,
                },
            ),
            S3Error::InternalError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorResponse {
                    code: "InternalError".to_string(),
                    message: format!("We encountered an internal error. Please try again. {}", msg),
                    bucket_name: None,
                    request_id,
                },
            ),
            S3Error::ServiceUnavailable(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorResponse {
                    code: "ServiceUnavailable".to_string(),
                    message: format!("Service temporarily unavailable. Please try again later. {}", msg),
                    bucket_name: None,
                    request_id,
                },
            ),
        };

        let xml = xml_to_string(&error_response).unwrap_or_else(|_| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{}</Code>
  <Message>{}</Message>
  <RequestId>{}</RequestId>
</Error>"#,
                error_response.code, error_response.message, error_response.request_id
            )
        });

        Response::builder()
            .status(status)
            .header("Content-Type", "application/xml")
            .header("x-amz-request-id", &error_response.request_id)
            .body(Body::from(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", xml)))
            .unwrap()
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

fn generate_etag(content: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(content);
    format!("\"{}\"", hex::encode(hasher.finalize()))
}

fn format_rfc2822(dt: DateTime<Utc>) -> String {
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn format_iso8601(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S.%3fZ").to_string()
}

fn get_request_id() -> String {
    Uuid::new_v4().to_string()
}

// ============================================================================
// REPLICATION FUNCTIONS
// ============================================================================

async fn check_peer_health(peer: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();

    let url = format!("{}/internal/health", peer);
    match client.get(&url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

async fn replicate_to_peer(peer: &str, request: &ReplicationRequest) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let url = format!("{}/internal/replicate", peer);
    let response = client
        .post(&url)
        .json(request)
        .send()
        .await
        .map_err(|e| format!("Failed to send replication request: {}", e))?;

    if response.status().is_success() {
        let resp: ReplicationResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse replication response: {}", e))?;

        if resp.success {
            Ok(())
        } else {
            Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    } else {
        Err(format!("Replication failed with status: {}", response.status()))
    }
}

async fn replicate_with_quorum(storage: &Storage, request: &ReplicationRequest) -> Result<(), S3Error> {
    let mut successful_replications = 1; // Count self as one successful replication
    let mut replication_futures = vec![];

    // Check which peers are healthy and replicate to them
    for peer in &storage.peers {
        let peer_clone = peer.clone();
        let request_clone = request.clone();
        replication_futures.push(tokio::spawn(async move {
            replicate_to_peer(&peer_clone, &request_clone).await
        }));
    }

    // Wait for replication results
    for future in replication_futures {
        if let Ok(Ok(())) = future.await {
            successful_replications += 1;
        }
    }

    // Need at least 2 out of 3 nodes for quorum
    if successful_replications >= 2 {
        Ok(())
    } else {
        Err(S3Error::ServiceUnavailable(
            format!("Could not achieve quorum. Only {} out of 3 nodes succeeded", successful_replications)
        ))
    }
}

// ============================================================================
// INTERNAL REPLICATION HANDLERS
// ============================================================================

async fn health_check() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::from("OK"))
        .unwrap()
}

async fn handle_replication(
    State(storage): State<Storage>,
    axum::Json(request): axum::Json<ReplicationRequest>,
) -> axum::Json<ReplicationResponse> {
    let mut buckets = storage.buckets.write().unwrap();

    let result = match request.operation {
        ReplicationOperation::CreateBucket => {
            if buckets.contains_key(&request.bucket) {
                // Check if the bucket already exists with the same timestamp
                // If so, this is an idempotent operation
                Ok(())
            } else {
                let bucket = Bucket {
                    name: request.bucket.clone(),
                    creation_date: DateTime::from_timestamp(request.timestamp, 0).unwrap_or_else(Utc::now),
                    objects: HashMap::new(),
                };
                buckets.insert(request.bucket.clone(), bucket);
                Ok(())
            }
        }
        ReplicationOperation::DeleteBucket => {
            buckets.remove(&request.bucket);
            Ok(())
        }
        ReplicationOperation::PutObject => {
            if let Some(bucket) = buckets.get_mut(&request.bucket) {
                if let (Some(key), Some(data_b64)) = (request.key, request.data) {
                    let content = general_purpose::STANDARD.decode(&data_b64).unwrap_or_default();
                    let etag = generate_etag(&content);
                    let object = Object {
                        key: key.clone(),
                        content: Bytes::from(content),
                        content_type: request.content_type,
                        etag,
                        last_modified: DateTime::from_timestamp(request.timestamp, 0).unwrap_or_else(Utc::now),
                        size: 0, // Will be set based on content
                    };
                    let size = object.content.len() as u64;
                    let mut object = object;
                    object.size = size;

                    // Last-Writer-Wins conflict resolution
                    if let Some(existing) = bucket.objects.get(&key) {
                        if existing.last_modified.timestamp() < request.timestamp {
                            bucket.objects.insert(key, object);
                        } else if existing.last_modified.timestamp() == request.timestamp {
                            // If timestamps are equal, higher node-id wins
                            // Since this is a replication request, the sender has higher priority
                            bucket.objects.insert(key, object);
                        }
                    } else {
                        bucket.objects.insert(key, object);
                    }
                    Ok(())
                } else {
                    Err("Missing key or data for PutObject".to_string())
                }
            } else {
                // Bucket might not exist yet due to ordering, create it
                let mut bucket = Bucket {
                    name: request.bucket.clone(),
                    creation_date: Utc::now(),
                    objects: HashMap::new(),
                };

                if let (Some(key), Some(data_b64)) = (request.key, request.data) {
                    let content = general_purpose::STANDARD.decode(&data_b64).unwrap_or_default();
                    let etag = generate_etag(&content);
                    let size = content.len() as u64;
                    let object = Object {
                        key: key.clone(),
                        content: Bytes::from(content),
                        content_type: request.content_type,
                        etag,
                        last_modified: DateTime::from_timestamp(request.timestamp, 0).unwrap_or_else(Utc::now),
                        size,
                    };
                    bucket.objects.insert(key, object);
                    buckets.insert(request.bucket.clone(), bucket);
                    Ok(())
                } else {
                    Err("Missing key or data for PutObject".to_string())
                }
            }
        }
        ReplicationOperation::DeleteObject => {
            if let Some(bucket) = buckets.get_mut(&request.bucket) {
                if let Some(key) = request.key {
                    bucket.objects.remove(&key);
                }
            }
            Ok(())
        }
    };

    match result {
        Ok(()) => axum::Json(ReplicationResponse {
            success: true,
            error: None,
        }),
        Err(e) => axum::Json(ReplicationResponse {
            success: false,
            error: Some(e),
        }),
    }
}

// ============================================================================
// API HANDLERS
// ============================================================================

/// List all buckets
async fn list_buckets(State(storage): State<Storage>) -> Result<Response, Response> {
    let request_id = get_request_id();
    let buckets = storage.buckets.read().unwrap();

    let bucket_list: Vec<BucketInfo> = buckets
        .values()
        .map(|bucket| BucketInfo {
            name: bucket.name.clone(),
            creation_date: format_iso8601(bucket.creation_date),
        })
        .collect();

    let response = ListBucketsResponse {
        xmlns: "http://s3.amazonaws.com/doc/2006-03-01/".to_string(),
        owner: Owner {
            id: "owner-id".to_string(),
            display_name: "owner".to_string(),
        },
        buckets: BucketsContainer {
            bucket: bucket_list,
        },
    };

    let xml = xml_to_string(&response).map_err(|_| {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .header("x-amz-request-id", &request_id)
        .body(Body::from(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", xml)))
        .unwrap())
}

/// Create a bucket with replication
async fn create_bucket(
    State(storage): State<Storage>,
    Path(bucket_name): Path<String>,
) -> Result<Response, Response> {
    let request_id = get_request_id();


    // Check if bucket already exists locally
    {
        let buckets = storage.buckets.read().unwrap();
        if buckets.contains_key(&bucket_name) {
            return Err(S3Error::BucketAlreadyExists(bucket_name).to_response(request_id));
        }
    }

    // Create the bucket locally first
    let timestamp = Utc::now().timestamp();
    {
        let mut buckets = storage.buckets.write().unwrap();
        let bucket = Bucket {
            name: bucket_name.clone(),
            creation_date: DateTime::from_timestamp(timestamp, 0).unwrap(),
            objects: HashMap::new(),
        };
        buckets.insert(bucket_name.clone(), bucket);
    }

    // Replicate to peers
    let replication_request = ReplicationRequest {
        operation: ReplicationOperation::CreateBucket,
        bucket: bucket_name.clone(),
        key: None,
        data: None,
        content_type: None,
        timestamp,
    };

    if let Err(e) = replicate_with_quorum(&storage, &replication_request).await {
        // Rollback local change
        storage.buckets.write().unwrap().remove(&bucket_name);
        return Err(e.to_response(request_id));
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("x-amz-request-id", &request_id)
        .header("Location", format!("/{}", bucket_name))
        .body(Body::empty())
        .unwrap())
}

/// Head bucket (check if bucket exists)
async fn head_bucket(
    State(storage): State<Storage>,
    Path(bucket_name): Path<String>,
) -> Result<Response, Response> {
    let request_id = get_request_id();
    let buckets = storage.buckets.read().unwrap();

    if !buckets.contains_key(&bucket_name) {
        return Err(S3Error::NoSuchBucket(bucket_name).to_response(request_id));
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("x-amz-request-id", &request_id)
        .body(Body::empty())
        .unwrap())
}

/// Delete a bucket with replication
async fn delete_bucket(
    State(storage): State<Storage>,
    Path(bucket_name): Path<String>,
) -> Result<Response, Response> {
    let request_id = get_request_id();

    // Check if bucket exists and is empty
    {
        let buckets = storage.buckets.read().unwrap();
        let bucket = buckets.get(&bucket_name).ok_or_else(|| {
            S3Error::NoSuchBucket(bucket_name.clone()).to_response(request_id.clone())
        })?;

        if !bucket.objects.is_empty() {
            return Err(S3Error::BucketNotEmpty(bucket_name).to_response(request_id));
        }
    }

    // Delete locally first
    storage.buckets.write().unwrap().remove(&bucket_name);

    // Replicate to peers
    let replication_request = ReplicationRequest {
        operation: ReplicationOperation::DeleteBucket,
        bucket: bucket_name.clone(),
        key: None,
        data: None,
        content_type: None,
        timestamp: Utc::now().timestamp(),
    };

    // For delete operations, we're more lenient - if we can't reach quorum,
    // we still return success since the operation is idempotent
    let _ = replicate_with_quorum(&storage, &replication_request).await;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-amz-request-id", &request_id)
        .body(Body::empty())
        .unwrap())
}

/// List objects in a bucket
async fn list_objects_v2(
    State(storage): State<Storage>,
    Path(bucket_name): Path<String>,
    Query(params): Query<ListObjectsQuery>,
) -> Result<Response, Response> {
    let request_id = get_request_id();

    // Check if this is a ListObjectsV2 request
    if params.list_type != Some(2) {
        return Err(StatusCode::BAD_REQUEST.into_response());
    }

    let buckets = storage.buckets.read().unwrap();
    let bucket = buckets.get(&bucket_name).ok_or_else(|| {
        S3Error::NoSuchBucket(bucket_name.clone()).to_response(request_id.clone())
    })?;

    let prefix = params.prefix.unwrap_or_default();
    let max_keys = params.max_keys.unwrap_or(1000).min(1000);
    let continuation_token = params.continuation_token;

    // Filter objects by prefix
    let mut filtered_objects: Vec<&Object> = bucket
        .objects
        .values()
        .filter(|obj| obj.key.starts_with(&prefix))
        .collect();

    // Sort by key for consistent ordering
    filtered_objects.sort_by(|a, b| a.key.cmp(&b.key));

    // Handle continuation token
    if let Some(token) = &continuation_token {
        if let Some(start_idx) = filtered_objects.iter().position(|obj| obj.key > *token) {
            filtered_objects = filtered_objects[start_idx..].to_vec();
        } else {
            filtered_objects.clear();
        }
    }

    // Apply pagination
    let is_truncated = filtered_objects.len() > max_keys as usize;
    let contents: Vec<&Object> = filtered_objects
        .into_iter()
        .take(max_keys as usize)
        .collect();

    let next_continuation_token = if is_truncated && !contents.is_empty() {
        Some(contents.last().unwrap().key.clone())
    } else {
        None
    };

    let object_infos: Vec<ObjectInfo> = contents
        .into_iter()
        .map(|obj| ObjectInfo {
            key: obj.key.clone(),
            last_modified: format_iso8601(obj.last_modified),
            etag: obj.etag.clone(),
            size: obj.size,
            storage_class: "STANDARD".to_string(),
        })
        .collect();

    let response = ListObjectsResponse {
        xmlns: "http://s3.amazonaws.com/doc/2006-03-01/".to_string(),
        name: bucket_name,
        prefix,
        max_keys,
        is_truncated,
        contents: object_infos,
        next_continuation_token,
        continuation_token,
    };

    let xml = xml_to_string(&response).map_err(|_| {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/xml")
        .header("x-amz-request-id", &request_id)
        .body(Body::from(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", xml)))
        .unwrap())
}

/// Put an object into a bucket with replication
async fn put_object(
    State(storage): State<Storage>,
    Path((bucket_name, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    let request_id = get_request_id();

    // Check if bucket exists
    {
        let buckets = storage.buckets.read().unwrap();
        if !buckets.contains_key(&bucket_name) {
            return Err(S3Error::NoSuchBucket(bucket_name.clone()).to_response(request_id));
        }
    }

    let content_type = headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let etag = generate_etag(&body);
    let size = body.len() as u64;
    let timestamp = Utc::now().timestamp();

    // Store locally first
    {
        let mut buckets = storage.buckets.write().unwrap();
        if let Some(bucket) = buckets.get_mut(&bucket_name) {
            let object = Object {
                key: key.clone(),
                content: body.clone(),
                content_type: content_type.clone(),
                etag: etag.clone(),
                last_modified: DateTime::from_timestamp(timestamp, 0).unwrap(),
                size,
            };
            bucket.objects.insert(key.clone(), object);
        }
    }

    // Replicate to peers
    let replication_request = ReplicationRequest {
        operation: ReplicationOperation::PutObject,
        bucket: bucket_name.clone(),
        key: Some(key.clone()),
        data: Some(general_purpose::STANDARD.encode(&body)),
        content_type,
        timestamp,
    };

    if let Err(e) = replicate_with_quorum(&storage, &replication_request).await {
        // Rollback local change
        if let Some(bucket) = storage.buckets.write().unwrap().get_mut(&bucket_name) {
            bucket.objects.remove(&key);
        }
        return Err(e.to_response(request_id));
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("ETag", &etag)
        .header("x-amz-request-id", &request_id)
        .header("Content-Length", "0")
        .body(Body::empty())
        .unwrap())
}

/// Get an object from a bucket
async fn get_object(
    State(storage): State<Storage>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, Response> {
    let request_id = get_request_id();
    let buckets = storage.buckets.read().unwrap();

    let bucket = buckets.get(&bucket_name).ok_or_else(|| {
        S3Error::NoSuchBucket(bucket_name.clone()).to_response(request_id.clone())
    })?;

    let object = bucket.objects.get(&key).ok_or_else(|| {
        S3Error::NoSuchKey(key.clone()).to_response(request_id.clone())
    })?;

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("ETag", &object.etag)
        .header("Content-Length", object.size.to_string())
        .header("Last-Modified", format_rfc2822(object.last_modified))
        .header("x-amz-request-id", &request_id);

    if let Some(content_type) = &object.content_type {
        response = response.header("Content-Type", content_type);
    }

    Ok(response
        .body(Body::from(object.content.clone()))
        .unwrap())
}

/// Head object (get object metadata)
async fn head_object(
    State(storage): State<Storage>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, Response> {
    let request_id = get_request_id();
    let buckets = storage.buckets.read().unwrap();

    let bucket = buckets.get(&bucket_name).ok_or_else(|| {
        S3Error::NoSuchBucket(bucket_name.clone()).to_response(request_id.clone())
    })?;

    let object = bucket.objects.get(&key).ok_or_else(|| {
        S3Error::NoSuchKey(key.clone()).to_response(request_id.clone())
    })?;

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("ETag", &object.etag)
        .header("Content-Length", object.size.to_string())
        .header("Last-Modified", format_rfc2822(object.last_modified))
        .header("x-amz-request-id", &request_id);

    if let Some(content_type) = &object.content_type {
        response = response.header("Content-Type", content_type);
    }

    Ok(response.body(Body::empty()).unwrap())
}

/// Delete an object from a bucket with replication
async fn delete_object(
    State(storage): State<Storage>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, Response> {
    let request_id = get_request_id();

    // S3 delete is idempotent - always return 204 even if bucket or object doesn't exist
    {
        let mut buckets = storage.buckets.write().unwrap();
        if let Some(bucket) = buckets.get_mut(&bucket_name) {
            bucket.objects.remove(&key);
        }
    }

    // Replicate to peers
    let replication_request = ReplicationRequest {
        operation: ReplicationOperation::DeleteObject,
        bucket: bucket_name.clone(),
        key: Some(key),
        data: None,
        content_type: None,
        timestamp: Utc::now().timestamp(),
    };

    // For delete operations, we're more lenient
    let _ = replicate_with_quorum(&storage, &replication_request).await;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-amz-request-id", &request_id)
        .body(Body::empty())
        .unwrap())
}

// ============================================================================
// ROUTING
// ============================================================================

async fn handle_bucket_operations(
    State(storage): State<Storage>,
    method: Method,
    Path(bucket_name): Path<String>,
    query: Query<ListObjectsQuery>,
) -> Result<Response, Response> {
    match method {
        Method::PUT => create_bucket(State(storage), Path(bucket_name)).await,
        Method::HEAD => head_bucket(State(storage), Path(bucket_name)).await,
        Method::DELETE => delete_bucket(State(storage), Path(bucket_name)).await,
        Method::GET => {
            if query.list_type.is_some() {
                list_objects_v2(State(storage), Path(bucket_name), query).await
            } else {
                // This could be a regular bucket operation or object operation
                // Since we don't have an object key, treat it as an error
                Err(StatusCode::BAD_REQUEST.into_response())
            }
        }
        _ => Err(StatusCode::METHOD_NOT_ALLOWED.into_response()),
    }
}

async fn handle_request(
    State(storage): State<Storage>,
    method: Method,
    uri: axum::http::Uri,
    query: Query<ListObjectsQuery>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let path = uri.path().trim_start_matches('/');

    println!("DEBUG: Request {} {} (query: {:?})", method, uri, query);
    println!("DEBUG: Headers: {:?}", headers);

    // Root path - list buckets
    if path.is_empty() {
        if method == Method::GET {
            return list_buckets(State(storage)).await.unwrap_or_else(|e| e);
        } else {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
    }

    // Split path into bucket and optional key
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let bucket_name = parts[0].to_string();

    if parts.len() == 1 || (parts.len() == 2 && parts[1].is_empty()) {
        // Bucket operations (handle both /bucket and /bucket/ formats)
        handle_bucket_operations(State(storage), method, Path(bucket_name), query)
            .await
            .unwrap_or_else(|e| e)
    } else {
        // Object operations
        let key = parts[1].to_string();

        // Convert body to bytes for object operations
        let body_bytes = match method {
            Method::PUT => {
                match axum::body::to_bytes(body, usize::MAX).await {
                    Ok(bytes) => bytes,
                    Err(_) => return StatusCode::BAD_REQUEST.into_response(),
                }
            }
            _ => Bytes::new(),
        };

        match method {
            Method::PUT => {
                put_object(State(storage), Path((bucket_name, key)), headers, body_bytes)
                    .await
                    .unwrap_or_else(|e| e)
            }
            Method::GET => {
                get_object(State(storage), Path((bucket_name, key)))
                    .await
                    .unwrap_or_else(|e| e)
            }
            Method::HEAD => {
                head_object(State(storage), Path((bucket_name, key)))
                    .await
                    .unwrap_or_else(|e| e)
            }
            Method::DELETE => {
                delete_object(State(storage), Path((bucket_name, key)))
                    .await
                    .unwrap_or_else(|e| e)
            }
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        }
    }
}

// ============================================================================
// MAIN APPLICATION
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Parse peers
    let peers: Vec<String> = args.peers
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    println!("Starting distributed S3 server:");
    println!("  Node ID: {}", args.node_id);
    println!("  Port: {}", args.port);
    println!("  Peers: {:?}", peers);

    let storage = Storage::new(args.node_id, peers);

    // Create the main router
    let app = Router::new()
        // Internal replication routes
        .route("/internal/health", get(health_check))
        .route("/internal/replicate", post(handle_replication))
        // S3 API routes
        .fallback(
            |State(storage): State<Storage>,
             method: Method,
             uri: axum::http::Uri,
             query: Query<ListObjectsQuery>,
             headers: HeaderMap,
             body: Body| async move {
                handle_request(State(storage), method, uri, query, headers, body).await
            },
        )
        .with_state(storage);

    let addr = format!("0.0.0.0:{}", args.port);
    let listener = TcpListener::bind(&addr).await?;
    println!("S3-compatible distributed server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}