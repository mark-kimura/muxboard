//! The list of registered projects, stored as JSON in the user's config directory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
}

impl Project {
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Project { name, path }
    }

    /// A tmux session name derived from the project name. tmux forbids ':' and '.' in names.
    pub fn session_name(&self) -> String {
        let s: String = self
            .name
            .chars()
            .map(|c| if c == ':' || c == '.' { '_' } else { c })
            .collect();
        let s = s.trim().to_string();
        if s.is_empty() { "project".into() } else { s }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tmux-gui")
        .join("projects.json")
}

pub fn load() -> Vec<Project> {
    let Ok(text) = std::fs::read_to_string(config_path()) else { return vec![] };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(projects: &[Project]) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(projects).map_err(|e| e.to_string())?;
    std::fs::write(&p, text).map_err(|e| format!("could not write {}: {e}", p.display()))
}

/// Canonical form used to compare a session's start folder with a project root.
pub fn normalize(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}
