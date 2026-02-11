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

/// Detect the current user's shell from `$SHELL`.
pub fn detect_shell() -> ShellType {
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        ShellType::Zsh
    } else if shell.contains("fish") {
        ShellType::Fish
    } else {
        // Default to bash for unknown shells
        ShellType::Bash
    }
}
