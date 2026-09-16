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

fn run(args: &[&str]) -> Result<String, String> {
    let out = Command::new("tmux")
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

/// Open a new terminal window attached to the session. Honours `$TERMINAL` if set.
pub fn attach_in_terminal(name: &str) -> Result<(), String> {
    let tmux_cmd = ["tmux", "attach-session", "-t", name];

    let mut candidates: Vec<(String, Vec<&str>)> = Vec::new();
    if let Ok(t) = std::env::var("TERMINAL") {
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
        return Err("no terminal emulator found (set $TERMINAL)".into());
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
