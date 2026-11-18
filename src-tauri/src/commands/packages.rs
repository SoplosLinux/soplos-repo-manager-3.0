// The packages of the repository in use: what it holds, the check of the
// packages that are dropped on the window (whole folders too) and adding or
// removing them with reprepro. The indexes are signed once, at the end, and
// not after each package.

use super::run::{run, JobLogger};
use super::{appstream, gpg, publish, repos};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;
use tauri_plugin_dialog::DialogExt;

const READ_TIMEOUT: Duration = Duration::from_secs(20);
// A kernel image is about 90 MB: copying it into the pool takes a moment.
const INCLUDE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepoPackage {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub size: u64,
    /// The first line of the description.
    pub summary: String,
    /// The file in the pool, relative to the repository.
    #[serde(skip)]
    pub filename: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageList {
    pub repo: String,
    pub architectures: Vec<String>,
    pub packages: Vec<RepoPackage>,
}

/// Reads the Packages indexes of the repository, which reprepro writes in
/// dists/<codename>/<component>/binary-<arch>/. A package of the architecture
/// "all" is in the index of every architecture, and is listed once.
fn read_indexes(dir: &Path) -> Vec<RepoPackage> {
    let layout = repos::layout(dir);
    let mut seen: HashMap<(String, String), RepoPackage> = HashMap::new();
    for component in &layout.components {
        for arch in &layout.architectures {
            let file = dir.join("dists").join(&layout.codename).join(component).join(format!("binary-{arch}")).join("Packages");
            let Ok(text) = fs::read_to_string(file) else { continue };
            for stanza in text.split("\n\n") {
                let field = |name: &str| {
                    stanza.lines().find_map(|l| l.strip_prefix(&format!("{name}: "))).unwrap_or_default().trim().to_string()
                };
                let name = field("Package");
                if name.is_empty() {
                    continue;
                }
                let package = RepoPackage {
                    version: field("Version"),
                    architecture: field("Architecture"),
                    size: field("Size").parse().unwrap_or(0),
                    summary: field("Description"),
                    filename: field("Filename"),
                    name,
                };
                seen.entry((package.name.clone(), package.architecture.clone())).or_insert(package);
            }
        }
    }
    let mut packages: Vec<RepoPackage> = seen.into_values().collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name).then(a.architecture.cmp(&b.architecture)));
    packages
}

#[tauri::command(async)]
pub fn list_packages() -> Result<PackageList, String> {
    let repo = repos::active()?;
    let dir = PathBuf::from(&repo.dir);
    Ok(PackageList { architectures: repos::layout(&dir).architectures, packages: read_indexes(&dir), repo: repo.name })
}

/// The order of two Debian versions, as dpkg sees it.
fn compare_versions(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    let less = run("dpkg", &["--compare-versions", a, "lt", b], None, None, READ_TIMEOUT).map(|r| r.ok).unwrap_or(false);
    if less {
        return Ordering::Less;
    }
    let equal = run("dpkg", &["--compare-versions", a, "eq", b], None, None, READ_TIMEOUT).map(|r| r.ok).unwrap_or(false);
    if equal {
        Ordering::Equal
    } else {
        Ordering::Greater
    }
}

/// Every .deb inside `path` (or `path` itself), with the folder it sits in,
/// written from the name of the dropped folder: "7.3.4/V1".
pub(crate) fn collect_debs(path: &Path, root_label: &str, out: &mut Vec<(PathBuf, String)>) {
    if path.is_file() {
        if path.extension().is_some_and(|x| x == "deb") {
            out.push((path.to_path_buf(), root_label.to_string()));
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            let name = entry.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            // Hidden folders (.git and the like) are not looked into.
            if !name.starts_with('.') {
                collect_debs(&entry, &format!("{root_label}/{name}"), out);
            }
        } else {
            collect_debs(&entry, root_label, out);
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub file: String,
    /// The folder it came from, from the dropped folder down.
    pub group: String,
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub size: u64,
    /// new, upgrade, same, older, duplicate, wrongArch, noSection or invalid.
    /// Only new and upgrade are added.
    pub status: String,
    /// The version the repository has now, when it has one.
    pub current: Option<String>,
}

/// Looks at the packages found in `paths` (files and folders, gone through
/// with every folder inside them) and says what adding each one would do.
#[tauri::command(async)]
pub fn scan_packages(paths: Vec<String>) -> Result<Vec<Candidate>, String> {
    let repo = repos::active()?;
    let dir = PathBuf::from(&repo.dir);
    let architectures = repos::layout(&dir).architectures;
    let current: HashMap<(String, String), String> =
        read_indexes(&dir).into_iter().map(|p| ((p.name, p.architecture), p.version)).collect();

    let mut found: Vec<(PathBuf, String)> = Vec::new();
    for path in &paths {
        let path = PathBuf::from(path);
        let label = if path.is_dir() {
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        } else {
            path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        };
        collect_debs(&path, &label, &mut found);
    }
    found.dedup_by(|a, b| a.0 == b.0);

    let mut candidates: Vec<Candidate> = found
        .into_iter()
        .map(|(path, group)| {
            let path_text = path.to_string_lossy().to_string();
            let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let fields = run("dpkg-deb", &["-f", &path_text, "Package", "Version", "Architecture", "Section", "Priority"], None, None, READ_TIMEOUT)
                .ok()
                .filter(|r| r.ok)
                .map(|r| r.stdout);
            let field = |text: &str, name: &str| {
                text.lines().find_map(|l| l.strip_prefix(&format!("{name}: "))).unwrap_or_default().trim().to_string()
            };
            let mut c = Candidate {
                path: path_text,
                file,
                group,
                name: String::new(),
                version: String::new(),
                architecture: String::new(),
                size,
                status: "invalid".to_string(),
                current: None,
            };
            if let Some(text) = fields {
                c.name = field(&text, "Package");
                c.version = field(&text, "Version");
                c.architecture = field(&text, "Architecture");
                if !c.name.is_empty() && !c.version.is_empty() {
                    // reprepro skips a package with no Section or Priority, with
                    // only a line in its output ("No section given"): it is
                    // marked here so the package is not lost in silence.
                    c.status = if field(&text, "Section").is_empty() || field(&text, "Priority").is_empty() {
                        "noSection".to_string()
                    } else {
                        String::new()
                    };
                }
            }
            c
        })
        .collect();

    // The same package twice: the highest version is the one that counts.
    let mut best: HashMap<(String, String), usize> = HashMap::new();
    for i in 0..candidates.len() {
        if candidates[i].status == "invalid" || candidates[i].status == "noSection" {
            continue;
        }
        let key = (candidates[i].name.clone(), candidates[i].architecture.clone());
        match best.get(&key).copied() {
            None => {
                best.insert(key, i);
            }
            Some(j) => {
                if compare_versions(&candidates[i].version, &candidates[j].version) == Ordering::Greater {
                    candidates[j].status = "duplicate".to_string();
                    best.insert(key, i);
                } else {
                    candidates[i].status = "duplicate".to_string();
                }
            }
        }
    }

    for c in candidates.iter_mut().filter(|c| c.status.is_empty()) {
        if c.architecture != "all" && !architectures.contains(&c.architecture) {
            c.status = "wrongArch".to_string();
            continue;
        }
        c.current = current.get(&(c.name.clone(), c.architecture.clone())).cloned();
        c.status = match &c.current {
            None => "new",
            Some(now) => match compare_versions(&c.version, now) {
                Ordering::Greater => "upgrade",
                Ordering::Equal => "same",
                Ordering::Less => "older",
            },
        }
        .to_string();
    }
    candidates.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
    Ok(candidates)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    repo: String,
    /// adding, store, signing or publishing.
    stage: String,
    done: usize,
    total: usize,
    file: String,
}

fn tell(app: &tauri::AppHandle, repo: &str, stage: &str, done: usize, total: usize, file: &str) {
    let _ = app.emit(
        "packages-progress",
        Progress { repo: repo.to_string(), stage: stage.to_string(), done, total, file: file.to_string() },
    );
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub file: String,
    pub error: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoResult {
    pub repo: String,
    pub added: usize,
    /// Packages that this repository already had at the same or a newer version.
    pub skipped: usize,
    pub failed: Vec<Failure>,
    pub published: bool,
    /// Why it was not published: no Git address, or the error of the push.
    pub publish_error: Option<String>,
    /// An error that stopped the work on this repository.
    pub error: Option<String>,
}

/// Imports the AppStream files of the packages of the repository that have no
/// metainfo in it yet, so none is left out of the software centers. Each file
/// of the pool is looked at once: the ones that are not applications are
/// written down in db/ (which is not published), and not opened again.
fn import_missing(dir: &Path, packages: &[RepoPackage]) {
    let record = dir.join("db").join("soplos-no-metainfo");
    let mut checked: Vec<String> = fs::read_to_string(&record).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
    let before = checked.len();
    for p in packages {
        let has = dir.join("appstream").join(format!("{}.metainfo.xml", p.name)).is_file();
        if has || p.filename.is_empty() || checked.contains(&p.filename) {
            continue;
        }
        let deb = dir.join(&p.filename);
        let _ = appstream::import_package(dir, &deb.to_string_lossy(), &p.name);
        if !dir.join("appstream").join(format!("{}.metainfo.xml", p.name)).is_file() {
            checked.push(p.filename.clone());
        }
    }
    if checked.len() != before {
        let _ = fs::create_dir_all(dir.join("db"));
        let _ = fs::write(&record, checked.join("\n") + "\n");
    }
}

/// Signs, prepares the data for the software centers and publishes a
/// repository without changing its packages.
#[tauri::command(async)]
pub fn republish(app: tauri::AppHandle, name: String) -> RepoResult {
    let mut result = RepoResult { repo: name.clone(), ..Default::default() };
    let work = repos::by_name(&name).and_then(|repo| finish(&app, &repo, "Update repository", &mut result));
    if let Err(e) = work {
        result.error = Some(e);
    }
    result
}

/// The signed indexes, the data for the software centers (DEP-11) made from
/// the packages, and the Release signed again with it. What Packages and an
/// accepted proposal both do before publishing.
pub(crate) fn export_with_store(dir: &Path, repo: &repos::Repo) -> Result<(), String> {
    repos::export_signed(dir)?;
    let packages = read_indexes(dir);
    import_missing(dir, &packages);
    let names: Vec<String> = packages.into_iter().map(|p| p.name).collect();
    let files = appstream::regenerate(dir, repo, &names)?;
    let key = gpg::key_for(&repo.gpg_key)?;
    appstream::sign_release(dir, &files, &key.fingerprint)
}

/// After its packages change: the indexes, the data for the software centers
/// (DEP-11), the signature and the publication, in that order.
fn finish(app: &tauri::AppHandle, repo: &repos::Repo, message: &str, result: &mut RepoResult) -> Result<(), String> {
    let logger = JobLogger::new_step(app.clone(), "publishTitle", &[("repo", &repo.name)]);
    logger.step("publishStart", &[("repo", &repo.name)]);
    let dir = PathBuf::from(&repo.dir);
    tell(app, &repo.name, "signing", 0, 0, "");
    logger.step("signExport", &[]);
    tell(app, &repo.name, "store", 0, 0, "");
    logger.step("appstream", &[]);
    export_with_store(&dir, repo)?;
    if repo.git_url.trim().is_empty() {
        result.publish_error = Some("no-git".to_string());
        logger.step("localOnly", &[]);
        logger.done(true);
        return Ok(());
    }
    tell(app, &repo.name, "publishing", 0, 0, "");
    logger.step("gitSync", &[]);
    match publish::publish(Some(&logger), repo, message) {
        Ok(()) => {
            result.published = true;
            logger.done(true);
        }
        Err(e) => {
            result.publish_error = Some(e.clone());
            logger.step("error", &[("error", &e.to_string())]);
            logger.done(false);
        }
    }
    Ok(())
}

/// Adds the packages to each of `repos`, publishes each one, and tells every
/// step to the window as "packages-progress". A repository gets only the
/// packages it does not have at the same or a newer version. A package that
/// fails does not stop the rest, and a repository that fails does not stop
/// the others.
#[tauri::command(async)]
pub fn add_packages(app: tauri::AppHandle, paths: Vec<String>, repos: Vec<String>) -> Vec<RepoResult> {
    // What each file is, read once for every repository.
    let debs: Vec<(String, String, Option<(String, String, String)>)> = paths
        .iter()
        .map(|path| {
            let file = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let fields = run("dpkg-deb", &["-f", path, "Package", "Version", "Architecture"], None, None, READ_TIMEOUT)
                .ok()
                .filter(|r| r.ok && path.ends_with(".deb"))
                .map(|r| {
                    let f = |n: &str| r.stdout.lines().find_map(|l| l.strip_prefix(&format!("{n}: "))).unwrap_or_default().trim().to_string();
                    (f("Package"), f("Version"), f("Architecture"))
                })
                .filter(|(p, v, _)| !p.is_empty() && !v.is_empty());
            (path.clone(), file, fields)
        })
        .collect();

    repos
        .iter()
        .map(|name| {
            let mut result = RepoResult { repo: name.clone(), ..Default::default() };
            let work = (|| -> Result<(), String> {
                let repo = repos::by_name(name)?;
                let dir = PathBuf::from(&repo.dir);
                let dir_text = dir.to_string_lossy().to_string();
                let layout = repos::layout(&dir);
                let current: HashMap<(String, String), String> =
                    read_indexes(&dir).into_iter().map(|p| ((p.name, p.architecture), p.version)).collect();
                let mut included = Vec::new();
                for (i, (path, file, fields)) in debs.iter().enumerate() {
                    tell(&app, name, "adding", i, debs.len(), file);
                    let Some((package, version, arch)) = fields else {
                        result.failed.push(Failure { file: file.clone(), error: "Not a valid package".to_string() });
                        continue;
                    };
                    let fits = arch == "all" || layout.architectures.contains(arch);
                    let newer = current
                        .get(&(package.clone(), arch.clone()))
                        .map_or(true, |now| compare_versions(version, now) == Ordering::Greater);
                    if !fits || !newer {
                        result.skipped += 1;
                        continue;
                    }
                    let args = ["-b", &dir_text, "--ignore=unknownfield", "--export=silent-never", "includedeb", &layout.codename, path];
                    match run("reprepro", &args, None, None, INCLUDE_TIMEOUT) {
                        Ok(r) if r.ok => {
                            result.added += 1;
                            included.push((path.clone(), package.clone()));
                        }
                        Ok(r) => result.failed.push(Failure { file: file.clone(), error: r.error("reprepro") }),
                        Err(e) => result.failed.push(Failure { file: file.clone(), error: e }),
                    }
                }
                if included.is_empty() {
                    return Ok(());
                }
                // The metainfo, icon and screenshots of each application, for the software centers.
                tell(&app, name, "store", 0, 0, "");
                for (path, package) in &included {
                    if let Err(e) = appstream::import_package(&dir, path, package) {
                        let file = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        result.failed.push(Failure { file, error: e });
                    }
                }
                let message = format!("Add {} packages", included.len());
                finish(&app, &repo, &message, &mut result)
            })();
            if let Err(e) = work {
                result.error = Some(e);
            }
            result
        })
        .collect()
}

/// Takes a package out of the repository in use, and publishes it again.
#[tauri::command(async)]
pub fn remove_package(app: tauri::AppHandle, name: String) -> Result<RepoResult, String> {
    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !valid {
        return Err("Not a name of a package".to_string());
    }
    let repo = repos::active()?;
    let dir = PathBuf::from(&repo.dir);
    let dir_text = dir.to_string_lossy().to_string();
    let codename = repos::layout(&dir).codename;
    let r = run("reprepro", &["-b", &dir_text, "--export=silent-never", "remove", &codename, &name], None, None, READ_TIMEOUT)?;
    if !r.ok {
        return Err(r.error("reprepro"));
    }
    let mut result = RepoResult { repo: repo.name.clone(), ..Default::default() };
    finish(&app, &repo, &format!("Remove {name}"), &mut result)?;
    Ok(result)
}

#[tauri::command]
pub async fn pick_deb_files(app: tauri::AppHandle) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Debian packages", &["deb"])
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|fp| fp.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debs_of_a_folder_are_found_with_their_subfolder() {
        let root = std::env::temp_dir().join(format!("rm3-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("7.3.4/V1")).unwrap();
        fs::create_dir_all(root.join("7.3.4/V2")).unwrap();
        fs::create_dir_all(root.join("7.3.4/.git")).unwrap();
        fs::write(root.join("7.3.4/V1/a_1_amd64.deb"), b"x").unwrap();
        fs::write(root.join("7.3.4/V2/b_1_amd64.deb"), b"x").unwrap();
        fs::write(root.join("7.3.4/.git/c_1_amd64.deb"), b"x").unwrap();
        fs::write(root.join("7.3.4/notes.txt"), b"x").unwrap();
        let mut found = Vec::new();
        collect_debs(&root.join("7.3.4"), "7.3.4", &mut found);
        let groups: Vec<&str> = found.iter().map(|(_, g)| g.as_str()).collect();
        assert_eq!(groups, vec!["7.3.4/V1", "7.3.4/V2"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn debian_versions_are_compared_as_dpkg_does() {
        assert_eq!(compare_versions("1.0.0-9", "1.0.1"), Ordering::Less);
        assert_eq!(compare_versions("1.0.0-10", "1.0.0-9"), Ordering::Greater);
        assert_eq!(compare_versions("3.0.0-beta", "3.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("3.0.0~beta", "3.0.0"), Ordering::Less);
        assert_eq!(compare_versions("2.0", "2.0"), Ordering::Equal);
    }
}
