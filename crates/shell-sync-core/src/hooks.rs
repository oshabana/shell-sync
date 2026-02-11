use crate::shell::ShellType;

/// Generate shell hooks for the given shell type that capture command history
/// and send it to the local daemon via Unix socket.
pub fn generate_hooks(shell: ShellType, socket_path: &str, session_id: &str) -> String {
    match shell {
        ShellType::Zsh => generate_zsh_hooks(socket_path, session_id),
        ShellType::Bash => generate_bash_hooks(socket_path, session_id),
        ShellType::Fish => generate_fish_hooks(socket_path, session_id),
    }
}

fn generate_zsh_hooks(socket_path: &str, session_id: &str) -> String {
    format!(
        r#"# Shell Sync history hooks for zsh
# Auto-generated — do not edit manually

_shell_sync_session_id="{session_id}"
_shell_sync_socket="{socket_path}"
_shell_sync_cmd_start=0

_shell_sync_preexec() {{
    _shell_sync_cmd_start=$EPOCHREALTIME
    _shell_sync_last_cmd="$1"
}}

_shell_sync_precmd() {{
    local exit_code=$?
    if [[ -n "$_shell_sync_last_cmd" && -S "$_shell_sync_socket" ]]; then
        local end=$EPOCHREALTIME
        local duration_ms=$(( (${{end%.*}} - ${{_shell_sync_cmd_start%.*}}) * 1000 + (10#${{end#*.}} - 10#${{_shell_sync_cmd_start#*.}}) / 1000 ))
        [[ $duration_ms -lt 0 ]] && duration_ms=0
        local payload
        payload=$(printf '{{"command":"%s","cwd":"%s","exit_code":%d,"duration_ms":%d,"session_id":"%s","shell":"zsh"}}' \
            "$(echo "$_shell_sync_last_cmd" | sed 's/\\/\\\\/g; s/"/\\"/g')" \
            "$(pwd | sed 's/\\/\\\\/g; s/"/\\"/g')" \
            "$exit_code" \
            "$duration_ms" \
            "$_shell_sync_session_id")
        echo "$payload" | nc -U -w1 "$_shell_sync_socket" 2>/dev/null &!
    fi
    _shell_sync_last_cmd=""
}}

autoload -Uz add-zsh-hook
add-zsh-hook preexec _shell_sync_preexec
add-zsh-hook precmd _shell_sync_precmd

# Ctrl+R: interactive history search via shell-sync TUI
__shell_sync_search() {{
    local selected
    selected=$(shell-sync search --inline </dev/tty 2>/dev/tty)
    if [[ -n "$selected" ]]; then
        LBUFFER="$selected"
        RBUFFER=""
    fi
    zle reset-prompt
}}
zle -N __shell_sync_search
bindkey '^R' __shell_sync_search
"#,
        session_id = session_id,
        socket_path = socket_path,
    )
}

fn generate_bash_hooks(socket_path: &str, session_id: &str) -> String {
    format!(
        r#"# Shell Sync history hooks for bash
# Auto-generated — do not edit manually

_shell_sync_session_id="{session_id}"
_shell_sync_socket="{socket_path}"
_shell_sync_cmd_start=0
_shell_sync_last_cmd=""

_shell_sync_debug_trap() {{
    if [[ -z "$_shell_sync_last_cmd" ]]; then
        _shell_sync_cmd_start=$SECONDS
        _shell_sync_last_cmd="$BASH_COMMAND"
    fi
}}

_shell_sync_prompt_command() {{
    local exit_code=$?
    if [[ -n "$_shell_sync_last_cmd" && -S "$_shell_sync_socket" ]]; then
        local end=$SECONDS
        local duration_ms=$(( (end - _shell_sync_cmd_start) * 1000 ))
        [[ $duration_ms -lt 0 ]] && duration_ms=0
        local payload
        payload=$(printf '{{"command":"%s","cwd":"%s","exit_code":%d,"duration_ms":%d,"session_id":"%s","shell":"bash"}}' \
            "$(echo "$_shell_sync_last_cmd" | sed 's/\\/\\\\/g; s/"/\\"/g')" \
            "$(pwd | sed 's/\\/\\\\/g; s/"/\\"/g')" \
            "$exit_code" \
            "$duration_ms" \
            "$_shell_sync_session_id")
        echo "$payload" | nc -U -w1 "$_shell_sync_socket" 2>/dev/null &
    fi
    _shell_sync_last_cmd=""
}}

trap '_shell_sync_debug_trap' DEBUG
PROMPT_COMMAND="_shell_sync_prompt_command${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}"

# Ctrl+R: interactive history search via shell-sync TUI
__shell_sync_search() {{
    local selected
    selected=$(shell-sync search --inline </dev/tty 2>/dev/tty)
    if [[ -n "$selected" ]]; then
        READLINE_LINE="$selected"
        READLINE_POINT=${{#READLINE_LINE}}
    fi
}}
bind -x '"\C-r": __shell_sync_search'
"#,
        session_id = session_id,
        socket_path = socket_path,
    )
}

fn generate_fish_hooks(socket_path: &str, session_id: &str) -> String {
    format!(
        r#"# Shell Sync history hooks for fish
# Auto-generated — do not edit manually

set -g _shell_sync_session_id "{session_id}"
set -g _shell_sync_socket "{socket_path}"
set -g _shell_sync_cmd_start 0

function _shell_sync_preexec --on-event fish_preexec
    set -g _shell_sync_cmd_start (date +%s)
    set -g _shell_sync_last_cmd $argv[1]
end

function _shell_sync_postexec --on-event fish_postexec
    set -l exit_code $status
    if test -n "$_shell_sync_last_cmd"; and test -S "$_shell_sync_socket"
        set -l end_time (date +%s)
        set -l duration_ms (math "($end_time - $_shell_sync_cmd_start) * 1000")
        if test $duration_ms -lt 0
            set duration_ms 0
        end
        set -l escaped_cmd (string replace -a '\\' '\\\\' -- "$_shell_sync_last_cmd" | string replace -a '"' '\\"')
        set -l escaped_cwd (string replace -a '\\' '\\\\' -- (pwd) | string replace -a '"' '\\"')
        set -l payload (printf '{{"command":"%s","cwd":"%s","exit_code":%d,"duration_ms":%d,"session_id":"%s","shell":"fish"}}' \
            "$escaped_cmd" \
            "$escaped_cwd" \
            $exit_code \
            $duration_ms \
            "$_shell_sync_session_id")
        echo "$payload" | nc -U -w1 "$_shell_sync_socket" 2>/dev/null &
    end
    set -g _shell_sync_last_cmd ""
end

# Ctrl+R: interactive history search via shell-sync TUI
function __shell_sync_search
    set -l selected (shell-sync search --inline </dev/tty 2>/dev/tty)
    if test -n "$selected"
        commandline -r -- $selected
    end
    commandline -f repaint
end
bind \cr __shell_sync_search
"#,
        session_id = session_id,
        socket_path = socket_path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_hooks_contain_preexec_precmd() {
        let hooks = generate_hooks(ShellType::Zsh, "/tmp/test.sock", "sess-123");
        assert!(hooks.contains("preexec"));
        assert!(hooks.contains("precmd"));
        assert!(hooks.contains("sess-123"));
        assert!(hooks.contains("/tmp/test.sock"));
    }

    #[test]
    fn bash_hooks_contain_debug_trap() {
        let hooks = generate_hooks(ShellType::Bash, "/tmp/test.sock", "sess-123");
        assert!(hooks.contains("DEBUG"));
        assert!(hooks.contains("PROMPT_COMMAND"));
        assert!(hooks.contains("sess-123"));
    }

    #[test]
    fn fish_hooks_contain_events() {
        let hooks = generate_hooks(ShellType::Fish, "/tmp/test.sock", "sess-123");
        assert!(hooks.contains("fish_preexec"));
        assert!(hooks.contains("fish_postexec"));
        assert!(hooks.contains("sess-123"));
    }

    #[test]
    fn hooks_include_socket_path() {
        let socket = "/home/user/.shell-sync/sock";
        for shell in [ShellType::Zsh, ShellType::Bash, ShellType::Fish] {
            let hooks = generate_hooks(shell, socket, "s1");
            assert!(
                hooks.contains(socket),
                "Shell {:?} missing socket path",
                shell
            );
        }
    }
}
