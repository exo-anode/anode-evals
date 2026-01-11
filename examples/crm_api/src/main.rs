//! CRM CRUD API
//!
//! This API manages people records with the following fields:
//! - id: UUID (auto-generated)
//! - first_name: String (required)
//! - last_name: String (required)
//! - email: Option<String> (optional)
//! - phone: Option<String> (optional)
//!
//! Required endpoints:
//! - POST /people - Create a new person
//! - GET /people - List all people
//! - GET /people/:id - Get a specific person
//! - PUT /people/:id - Update a person
//! - DELETE /people/:id - Delete a person
//!
//! The API should:
//! - Listen on port 3000
//! - Use JSON for request/response bodies
//! - Return appropriate HTTP status codes
//! - Store data in memory (no database required)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// A person in the CRM system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
}

/// Request body for creating a new person
#[derive(Debug, Deserialize)]
pub struct CreatePersonRequest {
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
}

/// Request body for updating a person
#[derive(Debug, Deserialize)]
pub struct UpdatePersonRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
}

/// Application state containing the in-memory store
pub type AppState = Arc<RwLock<HashMap<Uuid, Person>>>;

#[tokio::main]
async fn main() {
    // TODO: Initialize the application state (in-memory store)

    // TODO: Build the router with all required endpoints:
    // - POST /people
    // - GET /people
    // - GET /people/:id
    // - PUT /people/:id
    // - DELETE /people/:id

    // TODO: Start the server on port 3000
    // Hint: Use axum::serve() with a TcpListener bound to "0.0.0.0:3000"

    println!("CRM API server starting on port 3000...");

    // Remove this panic once you implement the server
    panic!("TODO: Implement the CRM API server");
}

// TODO: Implement handler functions for each endpoint

/// Create a new person
/// POST /people
/// Returns: 201 Created with the new person, or 400 Bad Request if validation fails
async fn create_person(
    State(_state): State<AppState>,
    Json(_payload): Json<CreatePersonRequest>,
) -> Result<(StatusCode, Json<Person>), StatusCode> {
    // TODO: Implement this handler
    // - Validate that first_name and last_name are not empty
    // - Generate a new UUID for the person
    // - Store the person in the state
    // - Return 201 Created with the new person
    Err(StatusCode::NOT_IMPLEMENTED)
}

/// List all people
/// GET /people
/// Returns: 200 OK with array of all people
async fn list_people(State(_state): State<AppState>) -> Json<Vec<Person>> {
    // TODO: Implement this handler
    // - Return all people from the state as a JSON array
    Json(vec![])
}

/// Get a specific person by ID
/// GET /people/:id
/// Returns: 200 OK with the person, or 404 Not Found
async fn get_person(
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
) -> Result<Json<Person>, StatusCode> {
    // TODO: Implement this handler
    // - Look up the person by ID
    // - Return 404 if not found
    // - Return 200 with the person if found
    Err(StatusCode::NOT_IMPLEMENTED)
}

/// Update a person
/// PUT /people/:id
/// Returns: 200 OK with updated person, or 404 Not Found
async fn update_person(
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
    Json(_payload): Json<UpdatePersonRequest>,
) -> Result<Json<Person>, StatusCode> {
    // TODO: Implement this handler
    // - Look up the person by ID
    // - Return 404 if not found
    // - Update only the fields that are Some in the request
    // - Return 200 with the updated person
    Err(StatusCode::NOT_IMPLEMENTED)
}

/// Delete a person
/// DELETE /people/:id
/// Returns: 204 No Content, or 404 Not Found
async fn delete_person(
    State(_state): State<AppState>,
    Path(_id): Path<Uuid>,
) -> StatusCode {
    // TODO: Implement this handler
    // - Look up the person by ID
    // - Return 404 if not found
    // - Remove the person from the state
    // - Return 204 No Content
    StatusCode::NOT_IMPLEMENTED
}
