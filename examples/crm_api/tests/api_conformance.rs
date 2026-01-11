//! API Conformance Tests for CRM CRUD API
//!
//! These tests verify that the CRM API implementation conforms to the specification.
//! DO NOT MODIFY THESE TESTS - implement the API to make them pass.
//!
//! Run with: cargo test --test api_conformance

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const BASE_URL: &str = "http://localhost:3000";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Person {
    id: Uuid,
    first_name: String,
    last_name: String,
    email: Option<String>,
    phone: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreatePersonRequest {
    first_name: String,
    last_name: String,
    email: Option<String>,
    phone: Option<String>,
}

#[derive(Debug, Serialize)]
struct UpdatePersonRequest {
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    phone: Option<String>,
}

/// Server guard that kills the server when dropped
struct ServerGuard {
    child: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start the server and wait for it to be ready
fn start_server() -> ServerGuard {
    // Build the project first
    let build_status = Command::new("cargo")
        .args(["build", "--release"])
        .status()
        .expect("Failed to build project");

    assert!(build_status.success(), "Failed to build the CRM API");

    // Start the server
    let child = Command::new("cargo")
        .args(["run", "--release"])
        .spawn()
        .expect("Failed to start server");

    // Wait for server to be ready (poll until it responds)
    let client = Client::new();
    let max_attempts = 30;
    for attempt in 0..max_attempts {
        thread::sleep(Duration::from_millis(200));
        if let Ok(_) = reqwest::blocking::get(format!("{}/people", BASE_URL)) {
            println!("Server ready after {} attempts", attempt + 1);
            break;
        }
        if attempt == max_attempts - 1 {
            panic!("Server did not start within {} seconds", max_attempts / 5);
        }
    }

    ServerGuard { child }
}

// ============================================================================
// CREATE (POST /people) Tests
// ============================================================================

#[test]
fn test_create_person_with_all_fields() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let request = CreatePersonRequest {
        first_name: "John".to_string(),
        last_name: "Doe".to_string(),
        email: Some("john.doe@example.com".to_string()),
        phone: Some("+1-555-123-4567".to_string()),
    };

    let response = client
        .post(format!("{}/people", BASE_URL))
        .json(&request)
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 201, "Expected 201 Created");

    let person: Person = response.json().expect("Failed to parse response");
    assert_eq!(person.first_name, "John");
    assert_eq!(person.last_name, "Doe");
    assert_eq!(person.email, Some("john.doe@example.com".to_string()));
    assert_eq!(person.phone, Some("+1-555-123-4567".to_string()));
    assert!(!person.id.is_nil(), "ID should be a valid UUID");
}

#[test]
fn test_create_person_required_fields_only() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let request = CreatePersonRequest {
        first_name: "Jane".to_string(),
        last_name: "Smith".to_string(),
        email: None,
        phone: None,
    };

    let response = client
        .post(format!("{}/people", BASE_URL))
        .json(&request)
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 201, "Expected 201 Created");

    let person: Person = response.json().expect("Failed to parse response");
    assert_eq!(person.first_name, "Jane");
    assert_eq!(person.last_name, "Smith");
    assert_eq!(person.email, None);
    assert_eq!(person.phone, None);
}

#[test]
fn test_create_person_with_email_only() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let request = CreatePersonRequest {
        first_name: "Bob".to_string(),
        last_name: "Wilson".to_string(),
        email: Some("bob@example.com".to_string()),
        phone: None,
    };

    let response = client
        .post(format!("{}/people", BASE_URL))
        .json(&request)
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 201);

    let person: Person = response.json().expect("Failed to parse response");
    assert_eq!(person.email, Some("bob@example.com".to_string()));
    assert_eq!(person.phone, None);
}

#[test]
fn test_create_person_with_phone_only() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let request = CreatePersonRequest {
        first_name: "Alice".to_string(),
        last_name: "Brown".to_string(),
        email: None,
        phone: Some("+1-555-999-8888".to_string()),
    };

    let response = client
        .post(format!("{}/people", BASE_URL))
        .json(&request)
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 201);

    let person: Person = response.json().expect("Failed to parse response");
    assert_eq!(person.email, None);
    assert_eq!(person.phone, Some("+1-555-999-8888".to_string()));
}

// ============================================================================
// READ (GET /people and GET /people/:id) Tests
// ============================================================================

#[test]
fn test_list_people_empty() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let response = client
        .get(format!("{}/people", BASE_URL))
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let people: Vec<Person> = response.json().expect("Failed to parse response");
    assert!(people.is_empty(), "Expected empty list initially");
}

#[test]
fn test_list_people_after_create() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    // Create a person
    let request = CreatePersonRequest {
        first_name: "Test".to_string(),
        last_name: "User".to_string(),
        email: None,
        phone: None,
    };

    client
        .post(format!("{}/people", BASE_URL))
        .json(&request)
        .send()
        .expect("Failed to create person");

    // List people
    let response = client
        .get(format!("{}/people", BASE_URL))
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let people: Vec<Person> = response.json().expect("Failed to parse response");
    assert_eq!(people.len(), 1);
    assert_eq!(people[0].first_name, "Test");
}

#[test]
fn test_get_person_by_id() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    // Create a person
    let request = CreatePersonRequest {
        first_name: "Charlie".to_string(),
        last_name: "Delta".to_string(),
        email: Some("charlie@example.com".to_string()),
        phone: None,
    };

    let create_response = client
        .post(format!("{}/people", BASE_URL))
        .json(&request)
        .send()
        .expect("Failed to create person");

    let created: Person = create_response.json().expect("Failed to parse response");

    // Get the person by ID
    let response = client
        .get(format!("{}/people/{}", BASE_URL, created.id))
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let person: Person = response.json().expect("Failed to parse response");
    assert_eq!(person.id, created.id);
    assert_eq!(person.first_name, "Charlie");
    assert_eq!(person.last_name, "Delta");
}

#[test]
fn test_get_person_not_found() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let fake_id = Uuid::new_v4();
    let response = client
        .get(format!("{}/people/{}", BASE_URL, fake_id))
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 404, "Expected 404 Not Found");
}

// ============================================================================
// UPDATE (PUT /people/:id) Tests
// ============================================================================

#[test]
fn test_update_person_all_fields() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    // Create a person
    let create_request = CreatePersonRequest {
        first_name: "Original".to_string(),
        last_name: "Name".to_string(),
        email: Some("original@example.com".to_string()),
        phone: Some("111-111-1111".to_string()),
    };

    let create_response = client
        .post(format!("{}/people", BASE_URL))
        .json(&create_request)
        .send()
        .expect("Failed to create person");

    let created: Person = create_response.json().expect("Failed to parse response");

    // Update the person
    let update_request = UpdatePersonRequest {
        first_name: Some("Updated".to_string()),
        last_name: Some("Person".to_string()),
        email: Some("updated@example.com".to_string()),
        phone: Some("222-222-2222".to_string()),
    };

    let response = client
        .put(format!("{}/people/{}", BASE_URL, created.id))
        .json(&update_request)
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let updated: Person = response.json().expect("Failed to parse response");
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.first_name, "Updated");
    assert_eq!(updated.last_name, "Person");
    assert_eq!(updated.email, Some("updated@example.com".to_string()));
    assert_eq!(updated.phone, Some("222-222-2222".to_string()));
}

#[test]
fn test_update_person_partial() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    // Create a person
    let create_request = CreatePersonRequest {
        first_name: "Keep".to_string(),
        last_name: "This".to_string(),
        email: Some("keep@example.com".to_string()),
        phone: Some("333-333-3333".to_string()),
    };

    let create_response = client
        .post(format!("{}/people", BASE_URL))
        .json(&create_request)
        .send()
        .expect("Failed to create person");

    let created: Person = create_response.json().expect("Failed to parse response");

    // Update only first_name
    let update_request = UpdatePersonRequest {
        first_name: Some("Changed".to_string()),
        last_name: None,
        email: None,
        phone: None,
    };

    let response = client
        .put(format!("{}/people/{}", BASE_URL, created.id))
        .json(&update_request)
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let updated: Person = response.json().expect("Failed to parse response");
    assert_eq!(updated.first_name, "Changed");
    assert_eq!(updated.last_name, "This", "Last name should be unchanged");
    assert_eq!(updated.email, Some("keep@example.com".to_string()), "Email should be unchanged");
    assert_eq!(updated.phone, Some("333-333-3333".to_string()), "Phone should be unchanged");
}

#[test]
fn test_update_person_not_found() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let fake_id = Uuid::new_v4();
    let update_request = UpdatePersonRequest {
        first_name: Some("Ghost".to_string()),
        last_name: None,
        email: None,
        phone: None,
    };

    let response = client
        .put(format!("{}/people/{}", BASE_URL, fake_id))
        .json(&update_request)
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 404, "Expected 404 Not Found");
}

// ============================================================================
// DELETE (DELETE /people/:id) Tests
// ============================================================================

#[test]
fn test_delete_person() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    // Create a person
    let create_request = CreatePersonRequest {
        first_name: "ToDelete".to_string(),
        last_name: "Person".to_string(),
        email: None,
        phone: None,
    };

    let create_response = client
        .post(format!("{}/people", BASE_URL))
        .json(&create_request)
        .send()
        .expect("Failed to create person");

    let created: Person = create_response.json().expect("Failed to parse response");

    // Delete the person
    let delete_response = client
        .delete(format!("{}/people/{}", BASE_URL, created.id))
        .send()
        .expect("Failed to send request");

    assert_eq!(delete_response.status(), 204, "Expected 204 No Content");

    // Verify person is deleted
    let get_response = client
        .get(format!("{}/people/{}", BASE_URL, created.id))
        .send()
        .expect("Failed to send request");

    assert_eq!(get_response.status(), 404, "Person should be deleted");
}

#[test]
fn test_delete_person_not_found() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    let fake_id = Uuid::new_v4();
    let response = client
        .delete(format!("{}/people/{}", BASE_URL, fake_id))
        .send()
        .expect("Failed to send request");

    assert_eq!(response.status(), 404, "Expected 404 Not Found");
}

// ============================================================================
// Integration Tests (Full CRUD Flow)
// ============================================================================

#[test]
fn test_full_crud_flow() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    // 1. Create
    let create_request = CreatePersonRequest {
        first_name: "Integration".to_string(),
        last_name: "Test".to_string(),
        email: Some("integration@test.com".to_string()),
        phone: Some("555-555-5555".to_string()),
    };

    let create_response = client
        .post(format!("{}/people", BASE_URL))
        .json(&create_request)
        .send()
        .expect("Failed to create");

    assert_eq!(create_response.status(), 201);
    let created: Person = create_response.json().unwrap();

    // 2. Read
    let read_response = client
        .get(format!("{}/people/{}", BASE_URL, created.id))
        .send()
        .expect("Failed to read");

    assert_eq!(read_response.status(), 200);
    let read: Person = read_response.json().unwrap();
    assert_eq!(read.first_name, "Integration");

    // 3. Update
    let update_request = UpdatePersonRequest {
        first_name: Some("Modified".to_string()),
        last_name: None,
        email: None,
        phone: None,
    };

    let update_response = client
        .put(format!("{}/people/{}", BASE_URL, created.id))
        .json(&update_request)
        .send()
        .expect("Failed to update");

    assert_eq!(update_response.status(), 200);
    let updated: Person = update_response.json().unwrap();
    assert_eq!(updated.first_name, "Modified");
    assert_eq!(updated.last_name, "Test"); // Unchanged

    // 4. Delete
    let delete_response = client
        .delete(format!("{}/people/{}", BASE_URL, created.id))
        .send()
        .expect("Failed to delete");

    assert_eq!(delete_response.status(), 204);

    // 5. Verify deleted
    let verify_response = client
        .get(format!("{}/people/{}", BASE_URL, created.id))
        .send()
        .expect("Failed to verify");

    assert_eq!(verify_response.status(), 404);
}

#[test]
fn test_multiple_people() {
    let _server = start_server();
    let client = reqwest::blocking::Client::new();

    // Create multiple people
    let names = vec![
        ("Alice", "Anderson"),
        ("Bob", "Brown"),
        ("Charlie", "Clark"),
    ];

    for (first, last) in &names {
        let request = CreatePersonRequest {
            first_name: first.to_string(),
            last_name: last.to_string(),
            email: None,
            phone: None,
        };

        let response = client
            .post(format!("{}/people", BASE_URL))
            .json(&request)
            .send()
            .expect("Failed to create");

        assert_eq!(response.status(), 201);
    }

    // List all people
    let response = client
        .get(format!("{}/people", BASE_URL))
        .send()
        .expect("Failed to list");

    assert_eq!(response.status(), 200);

    let people: Vec<Person> = response.json().unwrap();
    assert_eq!(people.len(), 3, "Should have 3 people");
}
