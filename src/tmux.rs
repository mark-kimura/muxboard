//! Thin wrappers around the `tmux` command-line tool.

use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub name: String,
    pub windows: u32,
    pub attached: bool,
    pub created: String,
    /// The folder the session was started in.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub index: u32,
    pub name: String,
    pub active: bool,
    pub command: String,
}

/// Full path of the tmux binary. An app started from a desktop menu or the macOS Dock does not get
/// the shell's PATH, so Homebrew's and other common install locations are tried as well.
pub fn tmux_bin() -> &'static str {
    static BIN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        if find_in_path("tmux") {
            return "tmux".to_string();
        }
        for p in ["/opt/homebrew/bin/tmux", "/usr/local/bin/tmux", "/home/linuxbrew/.linuxbrew/bin/tmux", "/usr/bin/tmux"] {
            if std::path::Path::new(p).is_file() {
                return p.to_string();
            }
        }
        "tmux".to_string()
    })
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = Command::new(tmux_bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run tmux: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("tmux exited with {}", out.status) } else { err })
    }
}

pub fn list_sessions() -> Result<Vec<Session>, String> {
    let fmt = "#{session_name}\t#{session_windows}\t#{session_attached}\t#{t:session_created}\t#{session_path}";
    let text = match run(&["list-sessions", "-F", fmt]) {
        Ok(t) => t,
        // "no server running" is not an error for our purposes: there are simply no sessions.
        Err(e) if e.contains("no server running") || e.contains("No such file") => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    Ok(text
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some(Session {
                name: f.next()?.to_string(),
                windows: f.next()?.parse().ok()?,
                attached: f.next()?.parse::<u32>().ok()? > 0,
                created: f.next().unwrap_or("").to_string(),
                path: f.next().unwrap_or("").to_string(),
            })
        })
        .collect())
}

/// How many lines of scrollback history to include in the preview, in addition to the visible screen.
pub const PREVIEW_HISTORY_LINES: &str = "-5000";

/// Plain-text snapshot of the active pane of the given target (session or session:window),
/// including scrollback history.
pub fn capture_pane(target: &str) -> Result<String, String> {
    let text = run(&["capture-pane", "-p", "-S", PREVIEW_HISTORY_LINES, "-t", target])?;
    Ok(text.trim_matches('\n').to_string())
}

pub fn rename_session(old: &str, new: &str) -> Result<(), String> {
    run(&["rename-session", "-t", old, new]).map(|_| ())
}

pub fn kill_session(name: &str) -> Result<(), String> {
    run(&["kill-session", "-t", name]).map(|_| ())
}

pub fn new_session(name: &str, dir: Option<&str>) -> Result<(), String> {
    let mut args = vec!["new-session", "-d", "-s", name];
    if let Some(d) = dir.filter(|d| !d.trim().is_empty()) {
        args.extend(["-c", d]);
    }
    run(&args).map(|_| ())
}

pub fn detach_clients(name: &str) -> Result<(), String> {
    run(&["detach-client", "-s", name]).map(|_| ())
}

/// Candidate terminal emulators, each with the arguments that precede the command to run.
const TERMINALS: &[(&str, &[&str])] = &[
    ("gnome-terminal", &["--"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("wezterm", &["start", "--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("tilix", &["-e"]),
    ("foot", &[]),
    ("x-terminal-emulator", &["-e"]),
    ("xterm", &["-e"]),
];

fn find_in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

/// Open a new terminal window attached to the session. Uses `preferred` if given, else `$TERMINAL`, else the first terminal found.
pub fn attach_in_terminal(name: &str, preferred: Option<&str>) -> Result<(), String> {
    let preferred = preferred.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    #[cfg(target_os = "macos")]
    {
        // Terminal.app and iTerm2 have no command-line flag to run a command in a new window,
        // so they are driven through AppleScript. Anything else is treated as a CLI terminal.
        let choice = preferred.clone().unwrap_or_else(|| {
            if std::path::Path::new("/Applications/iTerm.app").exists() { "iTerm".into() } else { "Terminal".into() }
        });
        let lower = choice.to_lowercase();
        if lower == "terminal" || lower == "terminal.app" {
            return mac_terminal_app(name);
        }
        if lower == "iterm" || lower == "iterm2" || lower == "iterm.app" {
            return mac_iterm(name);
        }
    }

    let tmux_cmd = [tmux_bin(), "attach-session", "-t", name];
    let mut candidates: Vec<(String, Vec<&str>)> = Vec::new();
    if let Some(t) = preferred.or_else(|| std::env::var("TERMINAL").ok()) {
        if !t.trim().is_empty() {
            let base = std::path::Path::new(&t)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| t.clone());
            let pre = TERMINALS
                .iter()
                .find(|(n, _)| *n == base)
                .map(|(_, p)| p.to_vec())
                .unwrap_or_else(|| vec!["-e"]);
            candidates.push((t, pre));
        }
    }
    for (bin, pre) in TERMINALS {
        if find_in_path(bin) {
            candidates.push((bin.to_string(), pre.to_vec()));
        }
    }

    let Some((bin, pre)) = candidates.into_iter().next() else {
        return Err("no terminal emulator found (set one in Settings or $TERMINAL)".into());
    };

    Command::new(&bin)
        .args(&pre)
        .args(tmux_cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start {bin}: {e}"))
}

// ---- windows ----

pub fn new_window(session: &str, name: Option<&str>) -> Result<(), String> {
    let mut args = vec!["new-window", "-t", session];
    if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
        args.extend(["-n", n]);
    }
    run(&args).map(|_| ())
}

pub fn rename_window(target: &str, new: &str) -> Result<(), String> {
    run(&["rename-window", "-t", target, new]).map(|_| ())
}

pub fn kill_window(target: &str) -> Result<(), String> {
    run(&["kill-window", "-t", target]).map(|_| ())
}

/// Make this window the active one in its session (what an attached terminal shows).
pub fn select_window(target: &str) -> Result<(), String> {
    run(&["select-window", "-t", target]).map(|_| ())
}

/// Windows of every session, grouped by session name (order preserved from tmux).
pub fn list_all_windows() -> Result<Vec<(String, Window)>, String> {
    let fmt = "#{session_name}\t#{window_index}\t#{window_name}\t#{window_active}\t#{pane_current_command}";
    let text = match run(&["list-windows", "-a", "-F", fmt]) {
        Ok(t) => t,
        Err(e) if e.contains("no server running") || e.contains("No such file") => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    Ok(text
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let session = f.next()?.to_string();
            Some((
                session,
                Window {
                    index: f.next()?.parse().ok()?,
                    name: f.next()?.to_string(),
                    active: f.next()? == "1",
                    command: f.next().unwrap_or("").to_string(),
                },
            ))
        })
        .collect())
}

/// Type a line into the target window, followed by Enter, as if typed at its keyboard.
pub fn send_line(target: &str, line: &str) -> Result<(), String> {
    // `-l` sends the text literally (no key-name interpretation), then a separate Enter.
    run(&["send-keys", "-t", target, "-l", line])?;
    run(&["send-keys", "-t", target, "Enter"]).map(|_| ())
}

/// The attach command as one shell line, with the session name single-quoted for the shell.
#[cfg(target_os = "macos")]
fn attach_shell_line(name: &str) -> String {
    let quoted = format!("'{}'", name.replace('\'', "'\\''"));
    format!("{} attach-session -t {quoted}", tmux_bin())
}

/// Escape a string for use inside an AppleScript double-quoted literal.
#[cfg(target_os = "macos")]
fn applescript_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn osascript(lines: &[String]) -> Result<(), String> {
    let mut cmd = Command::new("osascript");
    for l in lines {
        cmd.arg("-e").arg(l);
    }
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| format!("could not run osascript: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(target_os = "macos")]
fn mac_terminal_app(name: &str) -> Result<(), String> {
    let line = applescript_str(&attach_shell_line(name));
    osascript(&[
        "tell application \"Terminal\"".into(),
        "activate".into(),
        format!("do script \"{line}\""),
        "end tell".into(),
    ])
}

#[cfg(target_os = "macos")]
fn mac_iterm(name: &str) -> Result<(), String> {
    let line = applescript_str(&attach_shell_line(name));
    osascript(&[
        "tell application \"iTerm\"".into(),
        "activate".into(),
        "set w to (create window with default profile)".into(),
        format!("tell current session of w to write text \"{line}\""),
        "end tell".into(),
    ])
}
