use std::path::PathBuf;

/// Detected shell type for the current user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellType {
    Zsh,
    Bash,
    Fish,
}

impl ShellType {
    /// File extension for the alias output file.
    pub fn alias_extension(&self) -> &str {
        match self {
            ShellType::Fish => "fish",
            _ => "sh",
        }
    }

    /// The shell RC file to add a source line to.
    pub fn rc_file(&self) -> PathBuf {
        let home = directories::BaseDirs::new()
            .expect("Could not determine home directory")
            .home_dir()
            .to_path_buf();

        match self {
            ShellType::Zsh => home.join(".zshrc"),
            ShellType::Bash => home.join(".bashrc"),
            ShellType::Fish => home.join(".config/fish/conf.d/shell-sync.fish"),
        }
    }

    /// Generate the source line to add to the shell RC file.
    pub fn source_line(&self, alias_file: &str) -> String {
        match self {
            ShellType::Fish => format!("source \"{}\"", alias_file),
            _ => format!("[ -f \"{}\" ] && source \"{}\"", alias_file, alias_file),
        }
    }

    /// Format a single alias line for this shell type.
    pub fn format_alias(&self, name: &str, command: &str) -> String {
        match self {
            ShellType::Fish => {
                format!("alias {} '{}'", name, command.replace('\'', "\\'"))
            }
            _ => {
                let escaped = command.replace('\'', "'\\''");
                format!("alias {}='{}'", name, escaped)
            }
        }
    }
}

/// Detect shell type from a shell path string.
pub fn detect_shell_from(shell_path: &str) -> ShellType {
    if shell_path.contains("zsh") {
        ShellType::Zsh
    } else if shell_path.contains("fish") {
        ShellType::Fish
    } else {
        // Default to bash for unknown shells
        ShellType::Bash
    }
}

/// Detect the current user's shell from `$SHELL`.
pub fn detect_shell() -> ShellType {
    let shell = std::env::var("SHELL").unwrap_or_default();
    detect_shell_from(&shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_extension_sh() {
        assert_eq!(ShellType::Zsh.alias_extension(), "sh");
    }

    #[test]
    fn bash_extension_sh() {
        assert_eq!(ShellType::Bash.alias_extension(), "sh");
    }

    #[test]
    fn fish_extension_fish() {
        assert_eq!(ShellType::Fish.alias_extension(), "fish");
    }

    #[test]
    fn bash_format_simple() {
        assert_eq!(
            ShellType::Bash.format_alias("gs", "git status"),
            "alias gs='git status'"
        );
    }

    #[test]
    fn bash_format_escapes_quotes() {
        assert_eq!(
            ShellType::Bash.format_alias("say", "echo 'hi'"),
            r"alias say='echo '\''hi'\'''"
        );
    }

    #[test]
    fn fish_format_simple() {
        assert_eq!(
            ShellType::Fish.format_alias("gs", "git status"),
            "alias gs 'git status'"
        );
    }

    #[test]
    fn fish_format_escapes_quotes() {
        assert_eq!(
            ShellType::Fish.format_alias("say", "echo 'hi'"),
            r"alias say 'echo \'hi\''"
        );
    }

    #[test]
    fn zsh_source_line_has_guard() {
        let line = ShellType::Zsh.source_line("/tmp/aliases.sh");
        assert!(line.contains("[ -f"));
        assert!(line.contains("&& source"));
        assert!(line.contains("/tmp/aliases.sh"));
    }

    #[test]
    fn fish_source_line_plain() {
        let line = ShellType::Fish.source_line("/tmp/aliases.fish");
        assert_eq!(line, r#"source "/tmp/aliases.fish""#);
        assert!(!line.contains("[ -f"));
    }

    #[test]
    fn detect_shell_from_env() {
        assert_eq!(detect_shell_from("/bin/zsh"), ShellType::Zsh);
        assert_eq!(detect_shell_from("/usr/bin/fish"), ShellType::Fish);
        assert_eq!(detect_shell_from("/bin/bash"), ShellType::Bash);
        assert_eq!(detect_shell_from("/bin/sh"), ShellType::Bash);
        assert_eq!(detect_shell_from(""), ShellType::Bash);
    }
}
