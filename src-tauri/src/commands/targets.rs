// Targets: destination repositories for publishing and submitting proposals.
// Completely dynamic and configurable by the user. Stored in targets.json.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub id: String,
    pub name: String,
    pub description: String,
    pub git_url: String,
    pub branch: String,
}

fn config_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config")
}

fn targets_file() -> PathBuf {
    config_home().join("org.soplos.repomanager").join("targets.json")
}

pub fn load_targets() -> Vec<Target> {
    fs::read_to_string(targets_file())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_targets(targets: &[Target]) -> Result<(), String> {
    let path = targets_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(targets).map_err(|e| e.to_string())?;
    fs::write(&path, &raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_targets() -> Vec<Target> {
    load_targets()
}

#[tauri::command]
pub fn save_target(target: Target) -> Result<(), String> {
    if target.id.trim().is_empty() || target.git_url.trim().is_empty() {
        return Err("A destination needs an identifier and a valid Git URL".to_string());
    }
    let mut targets = load_targets();
    if let Some(pos) = targets.iter().position(|t| t.id == target.id) {
        targets[pos] = target;
    } else {
        targets.push(target);
    }
    save_targets(&targets)
}

#[tauri::command]
pub fn remove_target(id: String) -> Result<(), String> {
    let mut targets = load_targets();
    targets.retain(|t| t.id != id);
    save_targets(&targets)
}

pub fn find_target(target_ref: &str) -> Option<Target> {
    let all = list_targets();
    let clean = target_ref.trim().trim_end_matches(".git").to_lowercase();
    all.into_iter().find(|t| {
        t.id.eq_ignore_ascii_case(target_ref.trim())
            || t.name.eq_ignore_ascii_case(target_ref.trim())
            || t.git_url.trim_end_matches(".git").to_lowercase() == clean
    })
}
