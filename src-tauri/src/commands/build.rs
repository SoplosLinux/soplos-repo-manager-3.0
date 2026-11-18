// Build: a Debian package made from a program, with what Debian and the
// software centers expect of it. Three kinds of source:
//
// - "file": one program (a binary or a script), installed as /usr/bin/<name>.
// - "folder": a program with its files in a folder, installed as /opt/<name>
//   (the place of the FHS for self-contained programs), with a launcher in
//   /usr/bin/<name> that runs its entry file.
// - "tree": a folder laid out as the system (usr/, etc/, opt/...), copied as
//   it is; a DEBIAN/ folder in it keeps its maintainer scripts.
//
// On top, optionally, the desktop integration: the menu entry, the icon in
// the sizes of the hicolor theme and the AppStream metainfo, all named by the
// same application id so they find each other (what Check looks at). The
// control file is written from the form; dpkg-deb builds the package with
// root as owner, and the result goes through Check.

use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::check::{check_one, DebCheck};
use super::run::run;

const BUILD: Duration = Duration::from_secs(900);
const QUICK: Duration = Duration::from_secs(30);
/// The sizes of the hicolor theme a PNG icon is written in.
const ICON_SIZES: [u32; 5] = [32, 48, 64, 128, 256];

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopSpec {
    /// Reverse-DNS id: org.example.App. Names the .desktop, the icon and the metainfo.
    pub app_id: String,
    pub display_name: String,
    pub comment: String,
    pub categories: Vec<String>,
    pub terminal: bool,
    /// A PNG or SVG file.
    pub icon: String,
    /// SPDX, for the metainfo: GPL-3.0-or-later, MIT...
    pub license: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildSpec {
    /// "file", "folder" or "tree".
    pub kind: String,
    pub source: String,
    /// For "folder": the file to run, relative to the folder.
    pub entry: String,
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub section: String,
    pub priority: String,
    pub maintainer_name: String,
    pub maintainer_email: String,
    pub homepage: String,
    pub summary: String,
    pub description: String,
    pub depends: Vec<String>,
    pub recommends: Vec<String>,
    pub desktop: Option<DesktopSpec>,
    pub output_dir: String,
    /// Replace a package of the same name already in output_dir.
    pub overwrite: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub path: String,
    pub check: DebCheck,
}

/// The error the interface recognises to ask before replacing a package.
pub const EXISTS: &str = "EXISTS";

// ── Checks of the form ────────────────────────────────────────────────────────

/// Debian policy 5.6.1: lower case letters, digits, + - and ., at least two,
/// starting with a letter or a digit.
pub fn valid_name(name: &str) -> bool {
    name.len() >= 2
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
}

/// org.example.App: at least two parts, each a letter first, then letters, digits, _ or -.
pub fn valid_app_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            p.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// A relation field (Depends, Recommends) from the items of the form.
fn relation(items: &[String]) -> String {
    items.iter().map(|i| i.trim()).filter(|i| !i.is_empty()).collect::<Vec<_>>().join(", ")
}

/// A field with no line breaks, so nothing can add a field of its own to the control file.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate(spec: &BuildSpec) -> Result<(), String> {
    if !valid_name(&spec.name) {
        return Err(format!("Invalid package name: {}", spec.name));
    }
    let version_ok = !spec.version.trim().is_empty()
        && run("dpkg", &["--validate-version", spec.version.trim()], None, None, QUICK).map(|r| r.ok).unwrap_or(false);
    if !version_ok {
        return Err(format!("Invalid version: {}", spec.version));
    }
    if spec.architecture.trim().is_empty() || spec.architecture.contains(char::is_whitespace) {
        return Err("Choose an architecture".to_string());
    }
    if spec.section.trim().is_empty() || spec.priority.trim().is_empty() {
        return Err("The package needs a section and a priority".to_string());
    }
    if spec.maintainer_name.trim().is_empty() || !spec.maintainer_email.contains('@') {
        return Err("The maintainer needs a name and an e-mail address".to_string());
    }
    if spec.summary.trim().is_empty() {
        return Err("The package needs a summary".to_string());
    }
    let source = Path::new(&spec.source);
    match spec.kind.as_str() {
        "file" if !source.is_file() => return Err(format!("{} is not a file", spec.source)),
        "folder" | "tree" if !source.is_dir() => return Err(format!("{} is not a folder", spec.source)),
        "file" | "folder" | "tree" => {}
        other => return Err(format!("Unknown kind of source: {other}")),
    }
    if spec.kind == "folder" {
        let entry = spec.entry.trim();
        if entry.is_empty() || entry.starts_with('/') || entry.split('/').any(|p| p == "..") || !source.join(entry).is_file() {
            return Err(format!("The entry file {} is not in the folder", spec.entry));
        }
    }
    if let Some(d) = &spec.desktop {
        if !valid_app_id(&d.app_id) {
            return Err(format!("Invalid application id: {}", d.app_id));
        }
        if d.display_name.trim().is_empty() {
            return Err("The menu entry needs a name".to_string());
        }
        if !d.icon.is_empty() && !Path::new(&d.icon).is_file() {
            return Err(format!("{} is not a file", d.icon));
        }
    }
    if !Path::new(&spec.output_dir).is_dir() {
        return Err(format!("{} is not a folder", spec.output_dir));
    }
    Ok(())
}

// ── The package tree ──────────────────────────────────────────────────────────

fn write(path: &Path, text: &str, mode: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())
}

/// Copies a file or a folder as it is (links, modes), with cp -a.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let r = run("cp", &["-a", "--", &from.to_string_lossy(), &to.to_string_lossy()], None, None, BUILD)?;
    if r.ok { Ok(()) } else { Err(r.error("cp")) }
}

/// The launcher of a program in /opt: it runs the entry file with the arguments it gets.
fn launcher(name: &str, entry: &str) -> String {
    format!("#!/bin/sh\n# Launcher of {name}, written by Soplos Repo Manager.\nexec \"/opt/{name}/{entry}\" \"$@\"\n")
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn desktop_entry(spec: &BuildSpec, d: &DesktopSpec) -> String {
    let mut text = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={} %U\nIcon={}\nTerminal={}\n",
        one_line(&d.display_name),
        spec.name,
        d.app_id,
        if d.terminal { "true" } else { "false" }
    );
    if !d.comment.trim().is_empty() {
        text.push_str(&format!("Comment={}\n", one_line(&d.comment)));
    }
    let categories: Vec<&str> = d.categories.iter().map(|c| c.trim()).filter(|c| !c.is_empty()).collect();
    if !categories.is_empty() {
        text.push_str(&format!("Categories={};\n", categories.join(";")));
    }
    text
}

fn metainfo(spec: &BuildSpec, d: &DesktopSpec, date: &str) -> String {
    let paragraphs: String = spec
        .description
        .split("\n\n")
        .map(|p| one_line(p))
        .filter(|p| !p.is_empty())
        .map(|p| format!("    <p>{}</p>\n", xml(&p)))
        .collect();
    let description = if paragraphs.is_empty() { format!("    <p>{}</p>\n", xml(&one_line(&spec.summary))) } else { paragraphs };
    let license = if d.license.trim().is_empty() { String::new() } else { format!("  <project_license>{}</project_license>\n", xml(d.license.trim())) };
    let homepage = if spec.homepage.trim().is_empty() { String::new() } else { format!("  <url type=\"homepage\">{}</url>\n", xml(spec.homepage.trim())) };
    let categories: String = d.categories.iter().filter(|c| !c.trim().is_empty()).map(|c| format!("    <category>{}</category>\n", xml(c.trim()))).collect();
    let categories = if categories.is_empty() { String::new() } else { format!("  <categories>\n{categories}  </categories>\n") };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<component type=\"desktop-application\">\n  <id>{id}</id>\n  <metadata_license>CC0-1.0</metadata_license>\n{license}  <name>{name}</name>\n  <summary>{summary}</summary>\n  <description>\n{description}  </description>\n  <launchable type=\"desktop-id\">{id}.desktop</launchable>\n  <developer id=\"{dev_id}\">\n    <name>{developer}</name>\n  </developer>\n{homepage}{categories}  <provides>\n    <binary>{binary}</binary>\n  </provides>\n  <content_rating type=\"oars-1.1\"/>\n  <releases>\n    <release version=\"{version}\" date=\"{date}\"/>\n  </releases>\n</component>\n",
        id = xml(&d.app_id),
        name = xml(&one_line(&d.display_name)),
        summary = xml(&one_line(&spec.summary)),
        dev_id = xml(&d.app_id.rsplit_once('.').map(|(a, _)| a).unwrap_or(&d.app_id)),
        developer = xml(&one_line(&spec.maintainer_name)),
        binary = xml(&spec.name),
        version = xml(upstream_of(spec.version.trim())),
    )
}

/// The upstream part of a Debian version, which is what a release states.
fn upstream_of(version: &str) -> &str {
    let v = version.split_once(':').map(|(_, v)| v).unwrap_or(version);
    v.rsplit_once('-').map(|(u, _)| u).unwrap_or(v)
}

/// The Description field: the summary, then the long text, each line with a
/// space in front and the empty lines as " .".
fn description_field(summary: &str, long: &str) -> String {
    let mut out = format!("Description: {}\n", one_line(summary));
    for (i, paragraph) in long.split("\n\n").map(one_line).filter(|p| !p.is_empty()).enumerate() {
        if i > 0 {
            out.push_str(" .\n");
        }
        // Lines of about 72 characters, as Debian writes them.
        let mut line = String::new();
        for word in paragraph.split(' ') {
            if !line.is_empty() && line.len() + word.len() + 1 > 72 {
                out.push_str(&format!(" {line}\n"));
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() {
            out.push_str(&format!(" {line}\n"));
        }
    }
    out
}

fn control(spec: &BuildSpec, installed_kib: u64) -> String {
    let mut text = format!(
        "Package: {}\nVersion: {}\nArchitecture: {}\nMaintainer: {} <{}>\nInstalled-Size: {}\nSection: {}\nPriority: {}\n",
        spec.name,
        spec.version.trim(),
        spec.architecture.trim(),
        one_line(&spec.maintainer_name),
        one_line(&spec.maintainer_email),
        installed_kib,
        one_line(&spec.section),
        one_line(&spec.priority),
    );
    let depends = relation(&spec.depends);
    if !depends.is_empty() {
        text.push_str(&format!("Depends: {}\n", one_line(&depends)));
    }
    let recommends = relation(&spec.recommends);
    if !recommends.is_empty() {
        text.push_str(&format!("Recommends: {}\n", one_line(&recommends)));
    }
    if !spec.homepage.trim().is_empty() {
        text.push_str(&format!("Homepage: {}\n", one_line(&spec.homepage)));
    }
    text.push_str(&description_field(&spec.summary, &spec.description));
    text
}

/// The size of what the package installs, in KiB, as dpkg counts it.
fn installed_size(root: &Path) -> u64 {
    fn walk(dir: &Path) -> u64 {
        let Ok(entries) = fs::read_dir(dir) else { return 0 };
        entries
            .flatten()
            .map(|e| {
                let path = e.path();
                match fs::symlink_metadata(&path) {
                    Ok(m) if m.is_dir() => {
                        if path.file_name().is_some_and(|n| n == "DEBIAN") { 0 } else { 1 + walk(&path) }
                    }
                    Ok(m) => m.len().div_ceil(1024).max(1),
                    Err(_) => 0,
                }
            })
            .sum()
    }
    walk(root)
}

fn icons(root: &Path, d: &DesktopSpec) -> Result<(), String> {
    if d.icon.is_empty() {
        return Ok(());
    }
    let source = Path::new(&d.icon);
    let base = root.join("usr/share/icons/hicolor");
    if source.extension().is_some_and(|x| x.eq_ignore_ascii_case("svg")) {
        let dest = base.join("scalable/apps").join(format!("{}.svg", d.app_id));
        fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(source, &dest).map_err(|e| e.to_string())?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    }
    // PNG in every size, also from an SVG: some software centers want a PNG.
    gdk_pixbuf::Pixbuf::file_info(source).ok_or_else(|| format!("{} is not an image", d.icon))?;
    for size in ICON_SIZES {
        let dest = base.join(format!("{size}x{size}/apps")).join(format!("{}.png", d.app_id));
        fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(source, size as i32, size as i32, true).map_err(|e| e.to_string())?;
        pixbuf.savev(&dest, "png", &[]).map_err(|e| e.to_string())?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn today() -> String {
    run("date", &["+%Y-%m-%d"], None, None, QUICK).map(|r| r.stdout.trim().to_string()).unwrap_or_default()
}

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    base.join("org.soplos.repomanager").join(format!("build-{name}-{}", std::process::id()))
}

/// Lays out the package in `root`: the program, the desktop integration and DEBIAN/control.
fn assemble(spec: &BuildSpec, root: &Path) -> Result<(), String> {
    let source = Path::new(&spec.source);
    match spec.kind.as_str() {
        "file" => {
            let dest = root.join("usr/bin").join(&spec.name);
            copy(source, &dest)?;
            fs::set_permissions(&dest, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        "folder" => {
            let dest = root.join("opt").join(&spec.name);
            copy(source, &dest)?;
            let entry = dest.join(spec.entry.trim());
            let mode = fs::metadata(&entry).map(|m| m.permissions().mode()).unwrap_or(0o644);
            fs::set_permissions(&entry, fs::Permissions::from_mode(mode | 0o755)).map_err(|e| e.to_string())?;
            write(&root.join("usr/bin").join(&spec.name), &launcher(&spec.name, spec.entry.trim()), 0o755)?;
        }
        _ => {
            // The tree as it is; its own DEBIAN/control is replaced by the one of the form.
            fs::create_dir_all(root).map_err(|e| e.to_string())?;
            for entry in fs::read_dir(source).map_err(|e| e.to_string())?.flatten() {
                copy(&entry.path(), &root.join(entry.file_name()))?;
            }
        }
    }

    if let Some(d) = &spec.desktop {
        write(&root.join("usr/share/applications").join(format!("{}.desktop", d.app_id)), &desktop_entry(spec, d), 0o644)?;
        write(&root.join("usr/share/metainfo").join(format!("{}.metainfo.xml", d.app_id)), &metainfo(spec, d, &today()), 0o644)?;
        icons(root, d)?;
    }

    let size = installed_size(root);
    write(&root.join("DEBIAN/control"), &control(spec, size), 0o644)?;
    fs::set_permissions(root.join("DEBIAN"), fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Builds the package of the form and checks it.
#[tauri::command(async)]
pub fn build_package(spec: BuildSpec) -> Result<BuildResult, String> {
    validate(&spec)?;
    let file = format!("{}_{}_{}.deb", spec.name, spec.version.trim().replace(':', "%3a"), spec.architecture.trim());
    let output = Path::new(&spec.output_dir).join(&file);
    if output.exists() && !spec.overwrite {
        return Err(EXISTS.to_string());
    }

    let root = scratch(&spec.name);
    let _ = fs::remove_dir_all(&root);
    let built = assemble(&spec, &root).and_then(|_| {
        let r = run("dpkg-deb", &["--root-owner-group", "-Zxz", "--build", &root.to_string_lossy(), &output.to_string_lossy()], None, None, BUILD)?;
        if r.ok { Ok(()) } else { Err(r.error("dpkg-deb")) }
    });
    let _ = fs::remove_dir_all(&root);
    built?;

    let check = check_one(&output, String::new(), &[], false);
    Ok(BuildResult { path: output.to_string_lossy().to_string(), check })
}

/// The architecture of this machine, as Debian names it (amd64, arm64...).
#[tauri::command(async)]
pub fn host_architecture() -> String {
    run("dpkg", &["--print-architecture"], None, None, QUICK).map(|r| r.stdout.trim().to_string()).unwrap_or_else(|_| "amd64".to_string())
}

// ── The maintainer, remembered from one package to the next ───────────────────

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Maintainer {
    pub name: String,
    pub email: String,
}

fn maintainer_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("org.soplos.repomanager").join("maintainer.json")
}

#[tauri::command(async)]
pub fn load_maintainer() -> Maintainer {
    fs::read_to_string(maintainer_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

#[tauri::command(async)]
pub fn save_maintainer(name: String, email: String) -> Result<(), String> {
    let path = maintainer_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Maintainer { name: name.trim().to_string(), email: email.trim().to_string() }).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

// ── File choosers ─────────────────────────────────────────────────────────────

/// One file: the program, its entry file or its icon (`images` limits it to PNG and SVG).
#[tauri::command]
pub async fn pick_file(app: tauri::AppHandle, images: bool, start: Option<String>) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file();
        if images {
            dialog = dialog.add_filter("PNG, SVG", &["png", "svg"]);
        }
        if let Some(dir) = start.filter(|s| Path::new(s).is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.blocking_pick_file().and_then(|fp| fp.into_path().ok()).map(|p| p.to_string_lossy().to_string())
    })
    .await
    .unwrap_or(None)
}

/// Opens a folder in the file manager.
#[tauri::command(async)]
pub fn open_folder(path: String) -> Result<(), String> {
    if !Path::new(&path).is_dir() {
        return Err(format!("{path} is not a folder"));
    }
    std::process::Command::new("xdg-open").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> BuildSpec {
        BuildSpec {
            kind: "file".into(),
            name: "hello-tool".into(),
            version: "1.2.0-1".into(),
            architecture: "all".into(),
            section: "utils".into(),
            priority: "optional".into(),
            maintainer_name: "Ana".into(),
            maintainer_email: "ana@example.org".into(),
            summary: "Says hello".into(),
            description: "A small tool that says hello.\n\nIt has a second paragraph.".into(),
            depends: vec!["bash".into(), " ".into(), "coreutils (>= 9)".into()],
            ..Default::default()
        }
    }

    #[test]
    fn names_follow_the_policy() {
        assert!(valid_name("hello-tool"));
        assert!(valid_name("g++-12"));
        assert!(!valid_name("Hello"));
        assert!(!valid_name("-x"));
        assert!(!valid_name("a"));
        assert!(valid_app_id("org.example.Hello"));
        assert!(!valid_app_id("hello"));
        assert!(!valid_app_id("org..x"));
        assert!(!valid_app_id("org.example.1x"));
    }

    #[test]
    fn writes_the_control_file() {
        let text = control(&spec(), 12);
        assert!(text.contains("Package: hello-tool\nVersion: 1.2.0-1\nArchitecture: all\nMaintainer: Ana <ana@example.org>\nInstalled-Size: 12\nSection: utils\nPriority: optional\n"));
        assert!(text.contains("Depends: bash, coreutils (>= 9)\n"));
        assert!(text.contains("Description: Says hello\n A small tool that says hello.\n .\n It has a second paragraph.\n"));
    }

    #[test]
    fn a_field_cannot_add_lines() {
        let mut s = spec();
        s.summary = "Hi\nDepends: evil".into();
        assert!(control(&s, 1).contains("Description: Hi Depends: evil\n"));
    }

    #[test]
    fn desktop_files_name_each_other() {
        let mut s = spec();
        let d = DesktopSpec { app_id: "org.example.Hello".into(), display_name: "Hello".into(), categories: vec!["Utility".into()], license: "MIT".into(), ..Default::default() };
        s.desktop = Some(d.clone());
        let entry = desktop_entry(&s, &d);
        assert!(entry.contains("Exec=hello-tool %U\nIcon=org.example.Hello\n"));
        assert!(entry.contains("Categories=Utility;\n"));
        let meta = metainfo(&s, &d, "2026-11-18");
        assert!(meta.contains("<launchable type=\"desktop-id\">org.example.Hello.desktop</launchable>"));
        assert!(meta.contains("<release version=\"1.2.0\" date=\"2026-11-18\"/>"));
        assert!(meta.contains("<p>It has a second paragraph.</p>"));
        assert!(roxmltree::Document::parse(&meta).is_ok());
    }

    #[test]
    fn builds_a_package_that_passes_check() {
        let dir = std::env::temp_dir().join(format!("soplos-build-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::create_dir_all(dir.join("out")).unwrap();
        fs::write(dir.join("app/run.sh"), "#!/bin/sh\necho hello\n").unwrap();
        let mut s = spec();
        s.kind = "folder".into();
        s.source = dir.join("app").to_string_lossy().to_string();
        s.entry = "run.sh".into();
        s.output_dir = dir.join("out").to_string_lossy().to_string();
        s.desktop = Some(DesktopSpec { app_id: "org.example.Hello".into(), display_name: "Hello".into(), categories: vec!["Utility".into()], license: "MIT".into(), ..Default::default() });
        let result = build_package(s.clone()).expect("build");
        assert!(result.path.ends_with("hello-tool_1.2.0-1_all.deb"));
        assert!(result.check.findings.iter().any(|f| f.code == "launchableOk"));
        assert!(!result.check.findings.iter().any(|f| f.level == "error"), "{:?}", result.check.findings);
        assert_eq!(build_package(s).unwrap_err(), EXISTS);
        fs::remove_dir_all(dir).unwrap();
    }
}
