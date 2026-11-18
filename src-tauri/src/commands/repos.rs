// The repositories the program keeps, in its own file of settings. The first
// time it runs, the profiles of the 2.x program are read and turned into
// repositories, so the repositories, folders and GPG keys already in use are
// there from the start. The file of 2.x is only read, never changed, and its
// Git tokens are not copied: tokens live in the keyring of the system.
//
// A repository on disk is a reprepro repository, as 2.x made them: its
// settings in conf/distributions, its database in db/, the packages in pool/
// and the signed indexes in dists/.

use super::gpg;
use super::run::run;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri_plugin_dialog::DialogExt;

// Signing the indexes can wait for the passphrase of the key.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(300);
pub const ARCHITECTURES: [&str; 4] = ["amd64", "arm64", "i386", "armhf"];

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    /// Short name, unique (it was the name of the profile in 2.x).
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// The local copy of the repository.
    pub dir: String,
    pub git_url: String,
    pub branch: String,
    /// Long ID or fingerprint of the GPG key that signs the repository.
    pub gpg_key: String,
    pub list_filename: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    active: Option<String>,
    repos: Vec<Repo>,
}

fn config_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config")
}

fn repos_file() -> PathBuf {
    config_home().join("org.soplos.repomanager").join("repos.json")
}

fn legacy_profiles_file() -> PathBuf {
    config_home().join("soplos-repo-manager").join("profiles.json")
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string()
}

/// The repositories of the profiles of 2.x, from its main profiles.json.
/// The files in its profiles/ folder are not read: one of them can disagree
/// with profiles.json, and profiles.json is the one the program kept up to date.
fn import_legacy() -> Store {
    let legacy = legacy_profiles_file();
    let Ok(raw) = fs::read_to_string(&legacy) else {
        return Store::default();
    };
    let Ok(serde_json::Value::Object(profiles)) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Store::default();
    };
    let mut repos: Vec<Repo> = profiles
        .iter()
        .filter(|(_, p)| p.is_object())
        .map(|(name, p)| Repo {
            name: name.clone(),
            title: text(p, "repo_title"),
            description: String::new(),
            dir: text(p, "repo_dir"),
            git_url: text(p, "git_url"),
            branch: text(p, "branch"),
            gpg_key: text(p, "gpg_key"),
            list_filename: text(p, "list_filename"),
        })
        .collect();
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    // The profile that 2.x had open is the active repository.
    let active = fs::read_to_string(legacy.with_file_name("config.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .map(|c| text(&c, "current_profile"))
        .filter(|name| repos.iter().any(|r| &r.name == name))
        .or_else(|| repos.first().map(|r| r.name.clone()));
    Store { active, repos }
}

fn save(store: &Store) -> Result<(), String> {
    let file = repos_file();
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    // Written next to the final file and renamed, so a cut never leaves half a file.
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &file).map_err(|e| e.to_string())
}

fn load_store() -> Store {
    if let Ok(raw) = fs::read_to_string(repos_file()) {
        if let Ok(store) = serde_json::from_str::<Store>(&raw) {
            return store;
        }
    }
    let store = import_legacy();
    if !store.repos.is_empty() {
        let _ = save(&store);
    }
    store
}

/// The repositories of the program. The first time, they come from 2.x.
pub fn load() -> Vec<Repo> {
    load_store().repos
}

/// The repository in use, which must be a reprepro repository on disk.
pub(crate) fn active() -> Result<Repo, String> {
    let store = load_store();
    let name = store.active.ok_or("There is no repository in use")?;
    let repo = store.repos.into_iter().find(|r| r.name == name).ok_or("There is no repository in use")?;
    if !Path::new(&repo.dir).join("conf/distributions").is_file() {
        return Err(format!("{} is not a reprepro repository", repo.dir));
    }
    Ok(repo)
}

/// A repository of the program by its name.
pub(crate) fn by_name(name: &str) -> Result<Repo, String> {
    let repo = load_store().repos.into_iter().find(|r| r.name == name).ok_or_else(|| format!("Unknown repository: {name}"))?;
    if !Path::new(&repo.dir).join("conf/distributions").is_file() {
        return Err(format!("{} is not a reprepro repository", repo.dir));
    }
    Ok(repo)
}

/// A field of conf/distributions, as reprepro reads it.
pub(crate) fn conf_value(dir: &Path, field: &str) -> Option<String> {
    conf_field(dir, field)
}

/// The distribution, components and architectures of a reprepro repository.
pub(crate) struct Layout {
    pub codename: String,
    pub components: Vec<String>,
    pub architectures: Vec<String>,
}

pub(crate) fn layout(dir: &Path) -> Layout {
    let list = |field: &str, fallback: &str| -> Vec<String> {
        conf_field(dir, field).unwrap_or_else(|| fallback.to_string()).split_whitespace().map(str::to_string).collect()
    };
    Layout {
        codename: conf_field(dir, "Codename").unwrap_or_else(|| "stable".to_string()),
        components: list("Components", "main"),
        architectures: list("Architectures", "amd64"),
    }
}

/// Signs the indexes again, after the packages or the settings change.
pub(crate) fn export_signed(dir: &Path) -> Result<(), String> {
    export(dir)
}

/// The value of `field` in the settings of reprepro (the first distribution).
fn conf_field(dir: &Path, field: &str) -> Option<String> {
    let raw = fs::read_to_string(dir.join("conf/distributions")).ok()?;
    let prefix = format!("{field}:");
    raw.lines().find_map(|l| l.strip_prefix(&prefix)).map(|v| v.trim().to_string())
}

/// `content` with the line of `field` set to `value`, in the first
/// distribution; added at its end when missing. The other lines, comments
/// included, are kept as they were.
fn with_conf_field(content: &str, field: &str, value: &str) -> String {
    let prefix = format!("{field}:");
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let first_end = lines.iter().position(|l| l.trim().is_empty()).unwrap_or(lines.len());
    match lines[..first_end].iter().position(|l| l.starts_with(&prefix)) {
        Some(i) => lines[i] = format!("{field}: {value}"),
        None => lines.insert(first_end, format!("{field}: {value}")),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn count_debs(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                count_debs(&path)
            } else {
                usize::from(path.extension().is_some_and(|x| x == "deb"))
            }
        })
        .sum()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoView {
    #[serde(flatten)]
    pub repo: Repo,
    /// The folder of the repository is there.
    pub exists: bool,
    /// The folder holds the settings of reprepro.
    pub managed: bool,
    pub packages: usize,
    /// The key that conf/distributions signs with.
    pub sign_with: String,
    pub architectures: Vec<String>,
    /// The date of the last signed indexes, as the Release file has it.
    pub released: Option<String>,
}

fn view(repo: Repo) -> RepoView {
    let dir = PathBuf::from(&repo.dir);
    let suite = conf_field(&dir, "Codename").unwrap_or_else(|| "stable".to_string());
    let released = fs::read_to_string(dir.join("dists").join(&suite).join("Release"))
        .ok()
        .and_then(|r| r.lines().find_map(|l| l.strip_prefix("Date:")).map(|d| d.trim().to_string()));
    RepoView {
        exists: dir.is_dir(),
        managed: dir.join("conf/distributions").is_file(),
        packages: count_debs(&dir.join("pool")),
        sign_with: conf_field(&dir, "SignWith").unwrap_or_default(),
        architectures: conf_field(&dir, "Architectures")
            .map(|a| a.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
        released,
        repo,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoList {
    pub active: Option<String>,
    pub repos: Vec<RepoView>,
}

#[tauri::command(async)]
pub fn list_repos() -> RepoList {
    let store = load_store();
    RepoList { active: store.active, repos: store.repos.into_iter().map(view).collect() }
}

#[tauri::command(async)]
pub fn set_active_repo(name: String) -> Result<(), String> {
    let mut store = load_store();
    if !store.repos.iter().any(|r| r.name == name) {
        return Err("Unknown repository".to_string());
    }
    store.active = Some(name);
    save(&store)
}

/// Signs the indexes again, after a change of its settings or key.
fn export(dir: &Path) -> Result<(), String> {
    let dir_text = dir.to_string_lossy().to_string();
    let run = run("reprepro", &["-b", &dir_text, "--ignore=unknownfield", "export"], None, None, EXPORT_TIMEOUT)?;
    if run.ok {
        Ok(())
    } else {
        Err(run.error("reprepro"))
    }
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRepo {
    pub name: String,
    pub title: String,
    pub description: String,
    pub dir: String,
    pub gpg_key: String,
    pub architectures: Vec<String>,
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The settings of reprepro for a new repository: one distribution, "stable",
/// with the component "main", as the repositories of 2.x.
fn distributions(title: &str, description: &str, architectures: &[String], fingerprint: &str) -> String {
    format!(
        "Origin: {title}\nLabel: {title}\nSuite: stable\nCodename: stable\nVersion: 1.0\nArchitectures: {}\nComponents: main\nDescription: {description}\nSignWith: {fingerprint}\n",
        architectures.join(" ")
    )
}

/// Makes a new repository in an empty folder, signed with a key of the keyring.
#[tauri::command(async)]
pub fn create_repo(repo: NewRepo) -> Result<(), String> {
    let mut store = load_store();
    let name = repo.name.trim().to_string();
    if !valid_name(&name) {
        return Err("The name can only have lowercase letters, digits and hyphens".to_string());
    }
    if store.repos.iter().any(|r| r.name == name) {
        return Err("There is already a repository with that name".to_string());
    }
    let title = single_line(&repo.title);
    if title.is_empty() {
        return Err("The repository needs a title".to_string());
    }
    let description = single_line(&repo.description);
    let archs: Vec<String> = ARCHITECTURES.iter().filter(|a| repo.architectures.iter().any(|x| x == *a)).map(|a| a.to_string()).collect();
    if archs.is_empty() {
        return Err("Choose at least one architecture".to_string());
    }
    let dir = PathBuf::from(repo.dir.trim());
    if !dir.is_absolute() {
        return Err("The folder must be a full path".to_string());
    }
    let existed = dir.exists();
    if existed && fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(true) {
        return Err("The folder must be new or empty".to_string());
    }
    let key = gpg::key_for(&repo.gpg_key)?;
    let public_key = gpg::public_key_armored(&key.fingerprint)?;

    let made = (|| -> Result<(), String> {
        for sub in ["conf", "pool/main", "appstream", "media"] {
            fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
        }
        fs::write(dir.join("conf/distributions"), distributions(&title, &description, &archs, &key.fingerprint)).map_err(|e| e.to_string())?;
        fs::write(dir.join("conf/options"), "basedir .\n").map_err(|e| e.to_string())?;
        // The database and the working folders of reprepro are not published.
        fs::write(dir.join(".gitignore"), "db/\nconf/\ntmp/\nincoming/\n").map_err(|e| e.to_string())?;
        fs::write(dir.join("public.key"), public_key).map_err(|e| e.to_string())?;
        export(&dir)
    })();
    if let Err(e) = made {
        // Nothing half made is left behind.
        if existed {
            for entry in ["conf", "pool", "appstream", "media", "db", "dists", ".gitignore", "public.key"] {
                let path = dir.join(entry);
                let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            }
        } else {
            let _ = fs::remove_dir_all(&dir);
        }
        return Err(e);
    }

    store.repos.push(Repo {
        name: name.clone(),
        title,
        description,
        dir: dir.to_string_lossy().to_string(),
        git_url: String::new(),
        branch: "main".to_string(),
        gpg_key: key.fingerprint,
        list_filename: format!("{name}.list"),
    });
    store.repos.sort_by(|a, b| a.name.cmp(&b.name));
    store.active = Some(name);
    save(&store)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoChanges {
    pub title: String,
    pub description: String,
    pub git_url: String,
    pub branch: String,
    pub list_filename: String,
    pub gpg_key: String,
}

/// Changes the settings of a repository. A new key or description is also
/// written in conf/distributions, and the indexes are signed again with it.
#[tauri::command(async)]
pub fn update_repo(name: String, changes: RepoChanges) -> Result<(), String> {
    let mut store = load_store();
    let Some(index) = store.repos.iter().position(|r| r.name == name) else {
        return Err("Unknown repository".to_string());
    };
    let title = single_line(&changes.title);
    if title.is_empty() {
        return Err("The repository needs a title".to_string());
    }
    let description = single_line(&changes.description);
    let old = store.repos[index].clone();
    let dir = PathBuf::from(&old.dir);
    let key_changed = !changes.gpg_key.trim().is_empty() && !changes.gpg_key.trim().eq_ignore_ascii_case(&old.gpg_key);
    let mut gpg_key = old.gpg_key.clone();

    if dir.join("conf/distributions").is_file() && (key_changed || description != old.description) {
        let conf_path = dir.join("conf/distributions");
        let mut conf = fs::read_to_string(&conf_path).map_err(|e| e.to_string())?;
        let backup = conf.clone();
        if key_changed {
            let key = gpg::key_for(&changes.gpg_key)?;
            conf = with_conf_field(&conf, "SignWith", &key.fingerprint);
            fs::write(dir.join("public.key"), gpg::public_key_armored(&key.fingerprint)?).map_err(|e| e.to_string())?;
            gpg_key = key.fingerprint;
        }
        if description != old.description {
            conf = with_conf_field(&conf, "Description", &description);
        }
        fs::write(&conf_path, &conf).map_err(|e| e.to_string())?;
        if let Err(e) = export(&dir) {
            // The repository stays as it was: settings back, and the old key published again.
            let _ = fs::write(&conf_path, backup);
            if key_changed {
                if let Ok(k) = gpg::key_for(&old.gpg_key).and_then(|k| gpg::public_key_armored(&k.fingerprint)) {
                    let _ = fs::write(dir.join("public.key"), k);
                }
            }
            return Err(e);
        }
    } else if key_changed {
        gpg_key = gpg::key_for(&changes.gpg_key)?.fingerprint;
    }

    store.repos[index] = Repo {
        name,
        title,
        description,
        dir: old.dir,
        git_url: changes.git_url.trim().to_string(),
        branch: changes.branch.trim().to_string(),
        gpg_key,
        list_filename: changes.list_filename.trim().to_string(),
    };
    save(&store)
}

/// Takes a repository out of the program. Its folder is left as it is.
#[tauri::command(async)]
pub fn remove_repo(name: String) -> Result<(), String> {
    let mut store = load_store();
    let before = store.repos.len();
    store.repos.retain(|r| r.name != name);
    if store.repos.len() == before {
        return Err("Unknown repository".to_string());
    }
    if store.active.as_deref() == Some(name.as_str()) {
        store.active = store.repos.first().map(|r| r.name.clone());
    }
    save(&store)
}

#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|fp| fp.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .unwrap_or(None)
}

/// Returns the raw.githubusercontent.com base URL for a repository.
fn raw_base(repo: &Repo) -> Option<String> {
    let url = repo.git_url.trim().trim_end_matches(".git").trim_end_matches('/');
    let branch = if repo.branch.trim().is_empty() { "main" } else { repo.branch.trim() };
    if let Some(path) = url.strip_prefix("https://github.com/") {
        Some(format!("https://raw.githubusercontent.com/{path}/{branch}"))
    } else {
        None
    }
}

/// Returns the codename used in conf/distributions (first Codename: line).
fn repo_codename(dir: &std::path::Path) -> String {
    fs::read_to_string(dir.join("conf/distributions"))
        .ok()
        .and_then(|conf| {
            conf.lines()
                .find_map(|l| l.strip_prefix("Codename:").map(|v| v.trim().to_string()))
        })
        .unwrap_or_else(|| "stable".to_string())
}

/// Returns the components from conf/distributions (first Components: line).
fn repo_components(dir: &std::path::Path) -> String {
    fs::read_to_string(dir.join("conf/distributions"))
        .ok()
        .and_then(|conf| {
            conf.lines()
                .find_map(|l| l.strip_prefix("Components:").map(|v| v.trim().to_string()))
        })
        .unwrap_or_else(|| "main".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallScript {
    /// The .sources file content (DEB822 format).
    pub sources_content: String,
    /// Filename to use for the .sources file, e.g. "soplos-kernels.sources".
    pub sources_filename: String,
    /// A ready-to-run bash install script.
    pub bash_script: String,
    /// The raw URL of the public GPG key on GitHub (for display / curl).
    pub key_url: String,
    /// The local path where apt expects the keyring, e.g. /usr/share/keyrings/soplos-kernels.gpg
    pub keyring_path: String,
}

/// Generates the DEB822 `.sources` file and the bash install script for a repository.
/// Returns an error if the repository has no git_url configured (the raw key URL cannot
/// be computed without it).
#[tauri::command(async)]
pub fn generate_install_script(name: String) -> Result<InstallScript, String> {
    let repo = by_name(&name)?;
    let dir = PathBuf::from(&repo.dir);

    let base = raw_base(&repo)
        .ok_or_else(|| "The repository has no GitHub URL: edit it and add the Git URL before making the script".to_string())?;

    let codename = repo_codename(&dir);
    let components = repo_components(&dir);

    // The list filename without extension becomes the keyring name.
    let stem = repo.list_filename
        .trim()
        .trim_end_matches(".list")
        .trim_end_matches(".sources");
    let stem = if stem.is_empty() { &repo.name } else { stem };

    let keyring_path = format!("/usr/share/keyrings/{stem}.gpg");
    let sources_filename = format!("{stem}.sources");
    let key_url = format!("{base}/public.key");

    let sources_content = format!(
        "# Generated by Soplos Repo Manager\n\
         # Format: DEB822\n\
         \n\
         Types: deb\n\
         URIs: {base}/\n\
         Suites: {codename}\n\
         Components: {components}\n\
         Signed-By: {keyring_path}\n"
    );

    let bash_script = format!(
        "#!/bin/bash\n\
         # Install script for {title}\n\
         # Generated by Soplos Repo Manager 3.0\n\
         set -euo pipefail\n\
         \n\
         KEY_URL=\"{key_url}\"\n\
         KEYRING=\"{keyring_path}\"\n\
         SOURCES_FILE=\"/etc/apt/sources.list.d/{sources_filename}\"\n\
         \n\
         echo \"Installing GPG key...\"\n\
         curl -fsSL \"$KEY_URL\" | gpg --dearmor | sudo tee \"$KEYRING\" > /dev/null\n\
         sudo chmod 644 \"$KEYRING\"\n\
         \n\
         echo \"Adding repository source...\"\n\
         sudo tee \"$SOURCES_FILE\" > /dev/null << 'SOURCES_EOF'\n\
         {sources_content}\
         SOURCES_EOF\n\
         \n\
         echo \"Updating package lists...\"\n\
         sudo apt update\n\
         \n\
         echo \"Done! The repository '{title}' is now available.\"\n",
        title = repo.title,
        key_url = key_url,
        keyring_path = keyring_path,
        sources_filename = sources_filename,
        sources_content = sources_content,
    );

    Ok(InstallScript {
        sources_content,
        sources_filename,
        bash_script,
        key_url,
        keyring_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_of_2x_gives_its_fields() {
        let p: serde_json::Value = serde_json::from_str(
            r#"{"git_url":"https://example.org/r.git","branch":"main","gpg_key":"ABCDEF0123456789",
                "git_token":"secret","repo_dir":"/tmp/r","repo_title":"R","list_filename":"r.list"}"#,
        )
        .unwrap();
        assert_eq!(text(&p, "gpg_key"), "ABCDEF0123456789");
        assert_eq!(text(&p, "missing"), "");
    }

    #[test]
    fn a_field_of_reprepro_is_changed_in_place_and_the_rest_is_kept() {
        let conf = "Origin: X\nSignWith: OLD\n# comment\nPull: stable\n";
        let out = with_conf_field(conf, "SignWith", "NEW");
        assert_eq!(out, "Origin: X\nSignWith: NEW\n# comment\nPull: stable\n");
        let added = with_conf_field("Origin: X\n\nCodename: other\n", "Description", "D");
        assert_eq!(added, "Origin: X\nDescription: D\n\nCodename: other\n");
    }

    #[test]
    fn names_of_repositories_are_short_and_safe() {
        assert!(valid_name("dev-tools"));
        assert!(valid_name("tyron2"));
        assert!(!valid_name("Dev"));
        assert!(!valid_name("-x"));
        assert!(!valid_name("a b"));
        assert!(!valid_name(""));
    }
}
