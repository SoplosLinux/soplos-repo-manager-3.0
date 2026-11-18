// GitHub accounts of the program. The list (an id and a label for each one,
// and which one is in use) is kept in credentials.json next to repos.json; the
// tokens themselves live only in the keyring of the system, through the Secret
// Service that GNOME Keyring, KWallet and KeePassXC provide. With no keyring an
// account cannot be added: the interface asks to install one.
//
// A token never goes on a command line or into a URL, so it does not show in
// `ps`, in the log of a job or in the .git/config of a repository: curl reads
// it from its standard input and git from an environment variable.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const SERVICE: &str = "org.soplos.repomanager";

/// The error the interface recognises to show how to install a keyring.
pub const NO_KEYRING: &str = "NO_KEYRING";

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    /// Only read, to move the tokens of earlier versions into the keyring.
    #[serde(default, skip_serializing)]
    pub token: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub active: Option<String>,
    /// The single token of the first versions, moved into the keyring.
    #[serde(default, skip_serializing)]
    pub github_token: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub id: String,
    pub label: String,
    pub masked_token: String,
    pub is_active: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitHubUser {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub html_url: Option<String>,
    pub public_repos: Option<u64>,
}

// ── Keyring ───────────────────────────────────────────────────────────────────

fn entry(id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, id).map_err(keyring_error)
}

fn keyring_error(e: keyring::Error) -> String {
    match e {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => NO_KEYRING.to_string(),
        other => other.to_string(),
    }
}

/// Whether a Secret Service answers. Reading an entry that does not exist is
/// the cheapest question: NoEntry means the keyring is there.
pub fn keyring_available() -> bool {
    match entry("probe").and_then(|e| e.get_password().map_err(keyring_error)) {
        Ok(_) => true,
        Err(e) => e != NO_KEYRING,
    }
}

fn store_token(id: &str, token: &str) -> Result<(), String> {
    entry(id)?.set_password(token).map_err(keyring_error)
}

fn read_token(id: &str) -> Option<String> {
    entry(id).ok()?.get_password().ok().filter(|t| !t.is_empty())
}

fn delete_token(id: &str) {
    if let Ok(e) = entry(id) {
        let _ = e.delete_credential();
    }
}

// ── File ──────────────────────────────────────────────────────────────────────

fn config_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config")
}

fn credentials_file() -> PathBuf {
    config_home().join("org.soplos.repomanager").join("credentials.json")
}

/// The accounts, with any token an earlier version left in the file moved
/// into the keyring (and the file written again without it). A token that
/// cannot be moved, because there is no keyring, stays where it was until
/// there is one.
pub fn load() -> Credentials {
    let mut creds: Credentials = fs::read_to_string(credentials_file())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    let mut moved = false;
    if !creds.github_token.is_empty() && creds.accounts.is_empty() {
        let id = Uuid::new_v4().to_string();
        creds.accounts.push(Account { id: id.clone(), label: "GitHub".to_string(), token: creds.github_token.clone() });
        creds.active = Some(id);
    }
    let mut pending = !creds.github_token.is_empty() && creds.accounts.iter().all(|a| a.token.is_empty());
    for account in creds.accounts.iter_mut().filter(|a| !a.token.is_empty()) {
        // Only taken out of the file once the keyring gives the same token back.
        if store_token(&account.id, &account.token).is_ok() && read_token(&account.id).as_deref() == Some(account.token.as_str()) {
            account.token.clear();
            moved = true;
        } else {
            pending = true;
        }
    }
    if moved && !pending {
        creds.github_token.clear();
        let _ = save_creds(&creds);
    }
    creds
}

fn save_creds(creds: &Credentials) -> Result<(), String> {
    let path = credentials_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(creds).map_err(|e| e.to_string())?;
    fs::write(&path, &raw).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn active_id(creds: &Credentials) -> Option<String> {
    creds.active.clone().filter(|id| creds.accounts.iter().any(|a| &a.id == id)).or_else(|| creds.accounts.first().map(|a| a.id.clone()))
}

fn mask_token(token: &str) -> String {
    if token.chars().count() <= 8 {
        "••••••••".to_string()
    } else {
        let start: String = token.chars().take(4).collect();
        let end: String = token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{start}…{end}")
    }
}

// ── Used by other modules ─────────────────────────────────────────────────────

/// The token of the account in use, from the keyring.
pub fn active_token() -> Option<String> {
    let creds = load();
    active_id(&creds).and_then(|id| read_token(&id))
}

/// A Git URL without the user and token an earlier version wrote into it:
/// https://x-access-token:TOKEN@github.com/a/b.git -> https://github.com/a/b.git
pub fn clean_url(url: &str) -> String {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("https://") {
        if let Some((userinfo, host)) = rest.split_once('@') {
            if !userinfo.contains('/') {
                return format!("https://{host}");
            }
        }
    }
    url.to_string()
}

/// The environment that lets git authenticate on github.com with the token
/// in use, as an HTTP header given through GIT_CONFIG_*: nothing on the
/// command line, nothing written to .git/config. Empty with no account.
pub fn git_auth_env() -> Vec<(String, String)> {
    let Some(token) = active_token() else { return Vec::new() };
    use base64::Engine;
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    vec![
        ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
        ("GIT_CONFIG_KEY_0".to_string(), "http.https://github.com/.extraheader".to_string()),
        ("GIT_CONFIG_VALUE_0".to_string(), format!("Authorization: Basic {basic}")),
    ]
}

/// The headers of a GitHub API request, one per line, for `curl -H @-`: the
/// token travels on the standard input of curl, not on its command line.
pub fn github_header_lines(token: Option<&str>) -> String {
    let mut lines = String::from("Accept: application/vnd.github+json\nUser-Agent: RepoManager/3.0\n");
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        lines.push_str(&format!("Authorization: Bearer {token}\n"));
    }
    lines
}

fn fetch_github_user(token: &str) -> Result<GitHubUser, String> {
    let headers = github_header_lines(Some(token));
    let res = super::run::run("curl", &["-sS", "-H", "@-", "https://api.github.com/user"], None, Some(&headers), std::time::Duration::from_secs(15))?;
    if !res.ok {
        return Err(res.error("GitHub"));
    }
    let val: serde_json::Value = serde_json::from_str(&res.stdout).map_err(|e| format!("Invalid answer from GitHub: {e}"))?;
    if let Some(msg) = val.get("message").and_then(|m| m.as_str()) {
        return Err(format!("GitHub: {msg}"));
    }
    let login = val.get("login").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if login.is_empty() {
        return Err("The GitHub user could not be read".to_string());
    }
    let text = |key: &str| val.get(key).and_then(|v| v.as_str()).map(|s| s.to_string());
    Ok(GitHubUser {
        login,
        name: text("name"),
        avatar_url: text("avatar_url"),
        html_url: text("html_url"),
        public_repos: val.get("public_repos").and_then(|v| v.as_u64()),
    })
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Whether the keyring of the system answers, so the interface can ask to
/// install one before an account is added.
#[tauri::command(async)]
pub fn has_keyring() -> bool {
    keyring_available()
}

/// The accounts: id, label, the token shortened and which one is in use.
#[tauri::command(async)]
pub fn list_accounts() -> Vec<AccountInfo> {
    let creds = load();
    let active = active_id(&creds);
    creds
        .accounts
        .iter()
        .map(|a| AccountInfo {
            id: a.id.clone(),
            label: a.label.clone(),
            masked_token: read_token(&a.id).map(|t| mask_token(&t)).unwrap_or_default(),
            is_active: active.as_deref() == Some(a.id.as_str()),
        })
        .collect()
}

/// Checks the token with GitHub and keeps it in the keyring. Returns the new id.
#[tauri::command(async)]
pub fn add_account(label: String, token: String) -> Result<String, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("The token is empty".to_string());
    }
    if !keyring_available() {
        return Err(NO_KEYRING.to_string());
    }
    fetch_github_user(&token)?;
    let mut creds = load();
    let id = Uuid::new_v4().to_string();
    store_token(&id, &token)?;
    creds.accounts.push(Account { id: id.clone(), label: label.trim().to_string(), token: String::new() });
    if active_id(&creds).is_none() || creds.active.is_none() {
        creds.active = Some(id.clone());
    }
    if let Err(e) = save_creds(&creds) {
        delete_token(&id);
        return Err(e);
    }
    Ok(id)
}

#[tauri::command(async)]
pub fn set_active_account(id: String) -> Result<(), String> {
    let mut creds = load();
    if !creds.accounts.iter().any(|a| a.id == id) {
        return Err(format!("Unknown account: {id}"));
    }
    creds.active = Some(id);
    save_creds(&creds)
}

/// Removes an account and its token from the keyring.
#[tauri::command(async)]
pub fn remove_account(id: String) -> Result<(), String> {
    let mut creds = load();
    creds.accounts.retain(|a| a.id != id);
    if creds.active.as_deref() == Some(&id) {
        creds.active = creds.accounts.first().map(|a| a.id.clone());
    }
    save_creds(&creds)?;
    delete_token(&id);
    Ok(())
}

#[tauri::command(async)]
pub fn rename_account(id: String, label: String) -> Result<(), String> {
    let mut creds = load();
    let account = creds.accounts.iter_mut().find(|a| a.id == id).ok_or_else(|| format!("Unknown account: {id}"))?;
    account.label = label.trim().to_string();
    save_creds(&creds)
}

/// The GitHub user of one account (the Test connection button).
#[tauri::command(async)]
pub fn get_account_user(id: String) -> Result<GitHubUser, String> {
    let token = read_token(&id).ok_or_else(|| if keyring_available() { format!("The account {id} has no token in the keyring") } else { NO_KEYRING.to_string() })?;
    fetch_github_user(&token)
}

/// The GitHub user of the account in use (the author of proposals).
#[tauri::command(async)]
pub fn get_github_user() -> Result<GitHubUser, String> {
    let token = active_token().ok_or("No GitHub account")?;
    fetch_github_user(&token)
}

/// Whether there is an account with its token, for Send.
#[tauri::command(async)]
pub fn has_github_token() -> bool {
    active_token().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_token_out_of_old_urls() {
        assert_eq!(clean_url("https://x-access-token:ghp_abc@github.com/a/b.git"), "https://github.com/a/b.git");
        assert_eq!(clean_url("https://github_pat_123@github.com/a/b.git"), "https://github.com/a/b.git");
        assert_eq!(clean_url("https://github.com/a/b.git"), "https://github.com/a/b.git");
        assert_eq!(clean_url("git@github.com:a/b.git"), "git@github.com:a/b.git");
    }

    #[test]
    fn shortens_a_token_without_showing_it() {
        assert_eq!(mask_token("github_pat_11ABCDEF"), "gith…CDEF");
        assert_eq!(mask_token("short"), "••••••••");
    }

    // Talks to the real Secret Service of the session (only reads): run with --ignored.
    #[test]
    #[ignore]
    fn the_keyring_of_the_session_answers() {
        assert!(keyring_available());
    }

    #[test]
    fn headers_carry_the_token_only_when_there_is_one() {
        assert!(github_header_lines(Some("t0k")).contains("Authorization: Bearer t0k"));
        assert!(!github_header_lines(None).contains("Authorization"));
    }
}
