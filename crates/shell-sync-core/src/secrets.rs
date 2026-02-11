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
    SECRET_PATTERNS
        .iter()
        .any(|pattern| pattern.is_match(&combined))
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

    #[test]
    fn detects_case_insensitive() {
        assert!(check_for_secrets("SECRET", "value"));
        assert!(check_for_secrets("Secret", "value"));
        assert!(check_for_secrets("sEcReT", "value"));
    }

    #[test]
    fn detects_auth_in_command() {
        assert!(check_for_secrets("deploy", "curl -H Authorization"));
    }

    #[test]
    fn detects_private_key() {
        assert!(check_for_secrets("set_private_key", "cat key.pem"));
    }

    #[test]
    fn detects_credential_in_command() {
        assert!(check_for_secrets("export", "CREDENTIAL=foo"));
    }

    #[test]
    fn allows_empty_strings() {
        assert!(!check_for_secrets("", ""));
    }
}
