// Check: what can be told about a .deb before it is published, with the
// reasons in the words of the interface. Each finding is a code (translated
// under check.items.<code>), a level (ok, warning or error) and the values its
// text needs. It looks at what reprepro and GitHub would refuse, at what the
// software centers need to show the program (the desktop entry, the AppStream
// metainfo and the icon, which must name each other), and runs lintian when it
// is installed. Nothing is changed: the package is only read.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::packages::collect_debs;
use super::repos;
use super::run::{run, JobLogger, IS_CANCELLED};

const READ: Duration = Duration::from_secs(60);
const LINTIAN: Duration = Duration::from_secs(300);
/// GitHub refuses files over 100 MiB and warns from 50 MiB.
const GITHUB_LIMIT: u64 = 100 * 1024 * 1024;
const GITHUB_WARN: u64 = 50 * 1024 * 1024;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub code: String,
    /// "ok", "warning" or "error"; "info" for what does not decide the
    /// result (lintian, which is pedantic: a long symlink in an icon theme or
    /// the libc a Tauri bundle does not declare count as its "errors").
    pub level: String,
    pub values: BTreeMap<String, String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DebCheck {
    pub path: String,
    pub file: String,
    /// The folder it came from, from the dropped folder down.
    pub group: String,
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub size: u64,
    pub depends: String,
    /// The worst level of the findings.
    pub level: String,
    pub findings: Vec<Finding>,
    /// The lines lintian wrote (E: and W: first), or None when it is not installed.
    pub lintian: Option<Vec<String>>,
}

fn finding(code: &str, level: &str, values: &[(&str, &str)]) -> Finding {
    Finding { code: code.to_string(), level: level.to_string(), values: values.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
}

/// The control fields, the first line of each (a Description goes on in the lines that follow).
fn control_fields(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            out.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    out
}

/// The paths a package installs, from `dpkg-deb -c`: "./usr/bin/x" -> "usr/bin/x".
fn contents(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| {
            let path: String = line.split_whitespace().skip(5).collect::<Vec<_>>().join(" ");
            let path = path.split(" -> ").next().unwrap_or("").trim_start_matches("./").trim_end_matches('/').to_string();
            (!path.is_empty()).then_some(path)
        })
        .collect()
}

/// The upstream part of a Debian version: no epoch, no revision.
fn upstream(version: &str) -> &str {
    let v = version.split_once(':').map(|(_, v)| v).unwrap_or(version);
    v.rsplit_once('-').map(|(u, _)| u).unwrap_or(v)
}

fn desktop_value(text: &str, key: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry {
            if let Some(v) = line.strip_prefix(&format!("{key}=")) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// The program an Exec line starts: "env X=1 /usr/bin/app %U" -> "/usr/bin/app".
fn exec_program(exec: &str) -> String {
    // Words as the Desktop Entry specification splits them: a quoted one may hold spaces.
    let mut words = Vec::new();
    let mut rest = exec.trim();
    while !rest.is_empty() {
        if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            words.push(quoted[..end].to_string());
            rest = quoted.get(end + 1..).unwrap_or("").trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            words.push(rest[..end].to_string());
            rest = rest[end..].trim_start();
        }
    }
    words.into_iter().find(|w| w != "env" && !w.contains('=')).unwrap_or_default()
}

/// The desktop entries, the metainfo and the icons a package carries, and what they say of each other.
fn integration(files: &[String], extracted: &Path, findings: &mut Vec<Finding>, version: &str) {
    let desktops: Vec<&String> = files.iter().filter(|f| f.starts_with("usr/share/applications/") && f.ends_with(".desktop")).collect();
    let metainfos: Vec<&String> =
        files.iter().filter(|f| (f.starts_with("usr/share/metainfo/") || f.starts_with("usr/share/appdata/")) && f.ends_with(".xml")).collect();

    if desktops.is_empty() && metainfos.is_empty() {
        // A library, a kernel, a theme: nothing to show in a software center.
        findings.push(finding("noDesktopApp", "ok", &[]));
        return;
    }

    for desktop in &desktops {
        let name = desktop.rsplit('/').next().unwrap_or(desktop);
        let text = fs::read_to_string(extracted.join(desktop)).unwrap_or_default();
        if let Some(exec) = desktop_value(&text, "Exec") {
            let program = exec_program(&exec);
            let inside = if program.starts_with('/') {
                files.iter().any(|f| format!("/{f}") == program)
            } else {
                files.iter().any(|f| f == &format!("usr/bin/{program}") || f == &format!("usr/games/{program}"))
            };
            if !program.is_empty() && !inside {
                findings.push(finding("execOutside", "warning", &[("desktop", name), ("program", &program)]));
            }
        }
        match desktop_value(&text, "Icon") {
            None => findings.push(finding("desktopNoIcon", "warning", &[("desktop", name)])),
            Some(icon) => {
                let found = if icon.starts_with('/') {
                    files.iter().any(|f| format!("/{f}") == icon)
                } else {
                    files.iter().any(|f| {
                        let file = f.rsplit('/').next().unwrap_or("");
                        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
                        stem == icon && (f.starts_with("usr/share/icons/") || f.starts_with("usr/share/pixmaps/"))
                    })
                };
                findings.push(if found {
                    finding("iconFound", "ok", &[("icon", &icon)])
                } else {
                    finding("iconMissing", "warning", &[("icon", &icon), ("desktop", name)])
                });
            }
        }
    }

    if metainfos.is_empty() {
        findings.push(finding("noMetainfo", "warning", &[]));
        return;
    }
    if desktops.is_empty() {
        findings.push(finding("noDesktop", "warning", &[]));
    }

    for metainfo in &metainfos {
        let name = metainfo.rsplit('/').next().unwrap_or(metainfo).to_string();
        let raw = fs::read_to_string(extracted.join(metainfo)).unwrap_or_default();
        let Ok(doc) = roxmltree::Document::parse(&raw) else {
            findings.push(finding("metainfoBroken", "error", &[("file", &name)]));
            continue;
        };
        let root = doc.root_element();
        let child = |tag: &str| root.children().find(|n| n.has_tag_name(tag));
        let id = child("id").and_then(|n| n.text()).unwrap_or("").trim().to_string();
        if id.is_empty() {
            findings.push(finding("metainfoNoId", "error", &[("file", &name)]));
        }

        // The software centers join the metainfo and the menu entry through
        // the launchable: it must name a desktop file of the package.
        let kind = root.attribute("type").unwrap_or("");
        if kind == "desktop-application" || kind == "desktop" {
            let launchable = root
                .children()
                .find(|n| n.has_tag_name("launchable") && n.attribute("type") == Some("desktop-id"))
                .and_then(|n| n.text())
                .map(|t| t.trim().to_string());
            match launchable {
                None => findings.push(finding("noLaunchable", "error", &[("file", &name)])),
                Some(target) => {
                    if desktops.iter().any(|d| d.rsplit('/').next() == Some(target.as_str())) {
                        findings.push(finding("launchableOk", "ok", &[("desktop", &target)]));
                    } else {
                        findings.push(finding("launchableMissing", "error", &[("desktop", &target), ("file", &name)]));
                    }
                }
            }
        }

        let screenshots = child("screenshots").map(|n| n.children().filter(|c| c.has_tag_name("screenshot")).count()).unwrap_or(0);
        findings.push(if screenshots == 0 {
            finding("noScreenshots", "warning", &[])
        } else {
            finding("screenshots", "ok", &[("count", &screenshots.to_string())])
        });

        let latest = child("releases").and_then(|r| r.children().find(|c| c.has_tag_name("release"))).and_then(|r| r.attribute("version")).map(str::to_string);
        match latest {
            None => findings.push(finding("noReleases", "warning", &[])),
            Some(release) if release != version && release != upstream(version) => {
                findings.push(finding("releaseMismatch", "warning", &[("release", &release), ("version", version)]))
            }
            Some(_) => findings.push(finding("releaseOk", "ok", &[])),
        }

        // What libappstream itself (Discover, GNOME Software) thinks of the file.
        let path = extracted.join(metainfo);
        if let Ok(r) = run("appstreamcli", &["validate", "--no-net", &path.to_string_lossy()], None, None, READ) {
            let errors = r.stdout.lines().filter(|l| l.starts_with("E:")).count();
            if errors > 0 {
                findings.push(finding("appstreamErrors", "error", &[("file", &name), ("count", &errors.to_string())]));
            } else {
                findings.push(finding("appstreamOk", "ok", &[("file", &name)]));
            }
        }
    }
}

fn lintian(path: &str) -> Option<Vec<String>> {
    run("lintian", &["--version"], None, None, READ).ok().filter(|r| r.ok)?;
    let r = run("lintian", &["--no-tag-display-limit", path], None, None, LINTIAN).ok()?;
    let mut lines: Vec<String> = r.stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
    // Errors first, then warnings, then the rest (info, pedantic...).
    lines.sort_by_key(|l| if l.starts_with("E:") { 0 } else if l.starts_with("W:") { 1 } else { 2 });
    Some(lines)
}

fn scratch() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    base.join("org.soplos.repomanager").join(format!("check-{}-{n}", std::process::id()))
}

pub(crate) fn check_one(path: &Path, group: String, repo_architectures: &[String], with_lintian: bool) -> DebCheck {
    let path_text = path.to_string_lossy().to_string();
    let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut findings = Vec::new();
    let mut out = DebCheck {
        path: path_text.clone(),
        file,
        group,
        name: String::new(),
        version: String::new(),
        architecture: String::new(),
        size,
        depends: String::new(),
        level: "error".to_string(),
        findings: Vec::new(),
        lintian: None,
    };

    let fields = match run("dpkg-deb", &["-f", &path_text], None, None, READ) {
        Ok(r) if r.ok => control_fields(&r.stdout),
        _ => {
            out.findings.push(finding("notADeb", "error", &[]));
            return out;
        }
    };
    let get = |k: &str| fields.get(k).cloned().unwrap_or_default();
    out.name = get("Package");
    out.version = get("Version");
    out.architecture = get("Architecture");
    out.depends = get("Depends");

    for key in ["Package", "Version", "Architecture", "Description"] {
        if get(key).is_empty() {
            findings.push(finding("missingField", "error", &[("field", key)]));
        }
    }
    // reprepro skips a package without them, with only a line in its output.
    for key in ["Section", "Priority"] {
        if get(key).is_empty() {
            findings.push(finding("missingRepreproField", "error", &[("field", key)]));
        }
    }
    let maintainer = get("Maintainer");
    if maintainer.is_empty() || !maintainer.contains('<') {
        findings.push(finding("maintainer", "warning", &[]));
    }
    if !out.version.is_empty() {
        let valid = run("dpkg", &["--validate-version", &out.version], None, None, READ).map(|r| r.ok).unwrap_or(true);
        if !valid {
            findings.push(finding("badVersion", "error", &[("version", &out.version)]));
        }
    }
    if !out.architecture.is_empty() && out.architecture != "all" && !repo_architectures.is_empty() && !repo_architectures.contains(&out.architecture) {
        findings.push(finding("archNotInRepo", "warning", &[("arch", &out.architecture), ("list", &repo_architectures.join(", "))]));
    }
    let mib = format!("{:.1}", size as f64 / 1024.0 / 1024.0);
    if size > GITHUB_LIMIT {
        findings.push(finding("tooBig", "error", &[("size", &mib)]));
    } else if size > GITHUB_WARN {
        findings.push(finding("big", "warning", &[("size", &mib)]));
    }

    let files = run("dpkg-deb", &["-c", &path_text], None, None, READ).ok().filter(|r| r.ok).map(|r| contents(&r.stdout)).unwrap_or_default();
    let wants_files = files.iter().any(|f| f.starts_with("usr/share/applications/") || f.starts_with("usr/share/metainfo/") || f.starts_with("usr/share/appdata/"));
    if wants_files {
        let dir = scratch();
        let _ = fs::create_dir_all(&dir);
        if run("dpkg-deb", &["-x", &path_text, &dir.to_string_lossy()], None, None, READ).map(|r| r.ok).unwrap_or(false) {
            integration(&files, &dir, &mut findings, &out.version);
        } else {
            findings.push(finding("notExtracted", "warning", &[]));
        }
        let _ = fs::remove_dir_all(&dir);
    } else {
        integration(&files, Path::new("/nonexistent"), &mut findings, &out.version);
    }

    // lintian is slow (minutes on a kernel) and pedantic: only when asked for.
    out.lintian = if with_lintian { lintian(&path_text) } else { None };
    if !with_lintian {
    } else if let Some(lines) = &out.lintian {
        let errors = lines.iter().filter(|l| l.starts_with("E:")).count();
        let warnings = lines.iter().filter(|l| l.starts_with("W:")).count();
        findings.push(match (errors, warnings) {
            (0, 0) => finding("lintianClean", "ok", &[]),
            (0, w) => finding("lintianWarnings", "info", &[("count", &w.to_string())]),
            (e, w) => finding("lintianErrors", "info", &[("errors", &e.to_string()), ("warnings", &w.to_string())]),
        });
    } else {
        findings.push(finding("noLintian", "info", &[]));
    }

    out.level = if findings.iter().any(|f| f.level == "error") {
        "error"
    } else if findings.iter().any(|f| f.level == "warning") {
        "warning"
    } else {
        "ok"
    }
    .to_string();
    out.findings = findings;
    out
}

/// The .deb files found in `paths` (files and folders, with every folder inside).
fn gather(paths: &[String]) -> Vec<(PathBuf, String)> {
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    for path in paths {
        let path = PathBuf::from(path);
        let label = if path.is_dir() {
            path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        } else {
            path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        };
        collect_debs(&path, &label, &mut found);
    }
    found.dedup_by(|a, b| a.0 == b.0);
    found
}

/// Checks the packages found in `paths`, one after the other, with the
/// progress in the job bar. Cancelling stops between two packages and
/// returns what was checked until then.
#[tauri::command(async)]
pub fn check_packages(app: tauri::AppHandle, paths: Vec<String>, lintian: bool) -> Vec<DebCheck> {
    let architectures = repos::active().map(|r| repos::layout(Path::new(&r.dir)).architectures).unwrap_or_default();
    let found = gather(&paths);
    let total = found.len().to_string();
    let logger = JobLogger::new_step(app, "checkTitle", &[("count", &total)]);
    let mut out = Vec::new();
    for (i, (path, group)) in found.into_iter().enumerate() {
        if IS_CANCELLED.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        logger.step("checking", &[("n", &(i + 1).to_string()), ("total", &total), ("file", &file)]);
        out.push(check_one(&path, group, &architectures, lintian));
    }
    logger.done(true);
    out
}

/// The same without the job bar, for the test that reads real packages.
#[cfg(test)]
fn check_all(paths: &[String], lintian: bool) -> Vec<DebCheck> {
    let architectures = repos::active().map(|r| repos::layout(Path::new(&r.dir)).architectures).unwrap_or_default();
    gather(paths).into_iter().map(|(path, group)| check_one(&path, group, &architectures, lintian)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_control_fields() {
        let f = control_fields("Package: soplos-store\nVersion: 3.0.0\nDescription: Store\n Long text: with colon\nSection: admin\n");
        assert_eq!(f.get("Package").map(String::as_str), Some("soplos-store"));
        assert_eq!(f.get("Description").map(String::as_str), Some("Store"));
        assert_eq!(f.get("Section").map(String::as_str), Some("admin"));
        assert!(!f.contains_key("Long text"));
    }

    #[test]
    fn reads_the_paths_of_a_listing() {
        let listing = "drwxr-xr-x root/root         0 2026-11-16 20:47 ./usr/\n-rw-r--r-- root/root      1234 2026-11-16 20:47 ./usr/share/applications/soplos-store.desktop\nlrwxrwxrwx root/root         0 2026-11-16 20:47 ./usr/bin/x -> ../lib/x\n";
        assert_eq!(contents(listing), vec!["usr", "usr/share/applications/soplos-store.desktop", "usr/bin/x"]);
    }

    #[test]
    fn takes_the_upstream_version() {
        assert_eq!(upstream("3.0.0"), "3.0.0");
        assert_eq!(upstream("1:2.56-1"), "2.56");
        assert_eq!(upstream("12.9.6-0soplos1"), "12.9.6");
    }

    #[test]
    fn finds_the_program_of_an_exec_line() {
        assert_eq!(exec_program("env WEBKIT_DISABLE=1 /usr/bin/app %U"), "/usr/bin/app");
        assert_eq!(exec_program("soplos-store %F"), "soplos-store");
        assert_eq!(exec_program("\"/opt/My App/run\" --x"), "/opt/My App/run");
    }

    #[test]
    fn a_launchable_must_name_a_desktop_file_of_the_package() {
        let dir = std::env::temp_dir().join(format!("soplos-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("usr/share/metainfo")).unwrap();
        fs::create_dir_all(dir.join("usr/share/applications")).unwrap();
        fs::write(dir.join("usr/share/applications/app.desktop"), "[Desktop Entry]\nExec=app\nIcon=app\n").unwrap();
        fs::write(
            dir.join("usr/share/metainfo/org.x.app.metainfo.xml"),
            "<component type=\"desktop-application\"><id>org.x.app</id><launchable type=\"desktop-id\">org.x.app.desktop</launchable><releases><release version=\"1.0\"/></releases></component>",
        )
        .unwrap();
        let files: Vec<String> = ["usr/share/applications/app.desktop", "usr/share/metainfo/org.x.app.metainfo.xml", "usr/bin/app"].iter().map(|s| s.to_string()).collect();
        let mut findings = Vec::new();
        integration(&files, &dir, &mut findings, "1.0-1");
        assert!(findings.iter().any(|f| f.code == "launchableMissing" && f.level == "error"));
        assert!(findings.iter().any(|f| f.code == "iconMissing"));
        assert!(findings.iter().any(|f| f.code == "noScreenshots"));
        assert!(findings.iter().any(|f| f.code == "releaseOk"));
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod real {
    // Checks packages of the machine (only reads them): run with --ignored --nocapture.
    #[test]
    #[ignore]
    fn checks_real_packages() {
        let paths = std::env::var("CHECK_DEBS").unwrap_or_default();
        let list: Vec<String> = paths.split(':').filter(|p| !p.is_empty()).map(str::to_string).collect();
        for c in super::check_all(&list, std::env::var("CHECK_LINTIAN").is_ok()) {
            println!("== {} {} {} [{}]", c.name, c.version, c.architecture, c.level);
            for f in &c.findings {
                println!("   {} {} {:?}", f.level, f.code, f.values);
            }
            if let Some(l) = &c.lintian {
                println!("   lintian: {} lines, first: {:?}", l.len(), l.first());
            }
        }
    }
}
