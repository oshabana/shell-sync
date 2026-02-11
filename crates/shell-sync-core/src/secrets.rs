use regex::Regex;
use std::sync::LazyLock;

static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)password").unwrap(),
        Regex::new(r"(?i)secret").unwrap(),
        Regex::new(r"(?i)token").unwrap(),
        Regex::new(r"(?i)api[_-]?key").unwrap(),
        Regex::new(r"(?i)private[_-]?key").unwrap(),
        Regex::new(r"(?i)credential").unwrap(),
        Regex::new(r"(?i)auth").unwrap(),
    ]
});

/// Check if an alias name or command contains potential secrets.
pub fn check_for_secrets(alias_name: &str, command: &str) -> bool {
    let combined = format!("{} {}", alias_name, command);
    SECRET_PATTERNS.iter().any(|pattern| pattern.is_match(&combined))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_password() {
        assert!(check_for_secrets("db_password", "echo hunter2"));
    }

    #[test]
    fn detects_api_key() {
        assert!(check_for_secrets("set_api_key", "export KEY=abc"));
    }

    #[test]
    fn allows_safe_alias() {
        assert!(!check_for_secrets("gs", "git status"));
    }
}
