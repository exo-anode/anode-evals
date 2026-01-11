/// Returns a greeting message
///
/// This is the function that agents should implement.
/// The function should return "Hello, World!" exactly.
pub fn hello_world() -> String {
    // TODO: Agent should implement this
    String::new()
}

/// Returns a personalized greeting
///
/// This function should return "Hello, {name}!" where {name} is the input.
pub fn hello_name(name: &str) -> String {
    // TODO: Agent should implement this
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_world() {
        assert_eq!(hello_world(), "Hello, World!");
    }

    #[test]
    fn test_hello_world_not_empty() {
        assert!(!hello_world().is_empty());
    }

    #[test]
    fn test_hello_world_contains_hello() {
        assert!(hello_world().contains("Hello"));
    }

    #[test]
    fn test_hello_world_contains_world() {
        assert!(hello_world().contains("World"));
    }

    #[test]
    fn test_hello_name_alice() {
        assert_eq!(hello_name("Alice"), "Hello, Alice!");
    }

    #[test]
    fn test_hello_name_bob() {
        assert_eq!(hello_name("Bob"), "Hello, Bob!");
    }

    #[test]
    fn test_hello_name_empty() {
        assert_eq!(hello_name(""), "Hello, !");
    }

    #[test]
    fn test_hello_name_with_spaces() {
        assert_eq!(hello_name("John Doe"), "Hello, John Doe!");
    }
}
