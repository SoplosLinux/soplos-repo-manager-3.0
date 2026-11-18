// The AppStream data of a repository (DEP-11), which software centers such as
// Soplos Store, Discover and GNOME Software read. Everything comes from the
// metainfo, the icons and the screenshots inside the packages: nothing is
// made up. A package without metainfo or without an icon is left out.
//
// The layout is the one of 2.x, so the repositories keep working:
//   appstream/<package>.metainfo.xml
//   media/<package>/icons/<size>x<size>/<icon>.png
//   media/<package>/screenshots/<file>
//   dists/<codename>/<component>/dep11/Components-<arch>.yml(.gz), icons-<size>x<size>.tar

use super::repos::{self, Repo};
use super::run::run;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);
const ICON_SIZES: [u32; 3] = [48, 64, 128];

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// The PNG files of hicolor named `icon`, with the size of their folder
/// ("256x256@2" counts as 512), largest first. A scalable SVG counts as 1024.
fn icon_sources(root: &Path, icon: &str) -> Vec<(u32, PathBuf)> {
    let mut found = Vec::new();
    let Ok(sizes) = fs::read_dir(root.join("usr/share/icons/hicolor")) else { return found };
    for size in sizes.flatten() {
        let label = file_name(&size.path());
        let px = if label == "scalable" {
            1024
        } else {
            let (dims, scale) = label.split_once('@').unwrap_or((&label, "1"));
            let side: u32 = dims.split('x').next().and_then(|s| s.parse().ok()).unwrap_or(0);
            side * scale.parse::<u32>().unwrap_or(1)
        };
        for ext in ["png", "svg"] {
            let file = size.path().join("apps").join(format!("{icon}.{ext}"));
            if file.is_file() && px > 0 {
                found.push((px, file));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found
}

/// The icon a metainfo names: its stock icon, else the Icon= of its desktop
/// file in the package, else the name of the package.
fn icon_name(root: &Path, metainfo: &str, package: &str) -> String {
    let Ok(doc) = roxmltree::Document::parse(metainfo) else { return package.to_string() };
    let stock = doc
        .descendants()
        .find(|n| n.has_tag_name("icon") && n.attribute("type") == Some("stock"))
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(icon) = stock {
        return icon.to_string();
    }
    let desktop = doc.descendants().find(|n| n.has_tag_name("launchable")).and_then(|n| n.text()).map(str::trim);
    if let Some(desktop) = desktop {
        if let Ok(text) = fs::read_to_string(root.join("usr/share/applications").join(desktop)) {
            if let Some(icon) = text.lines().find_map(|l| l.strip_prefix("Icon=")) {
                return icon.trim().to_string();
            }
        }
    }
    package.to_string()
}

/// Writes `source` as a PNG of `size` x `size` at `dest`.
fn write_icon(source: &Path, size: u32, dest: &Path) -> Result<(), String> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(source, size as i32, size as i32, true).map_err(|e| e.to_string())?;
    pixbuf.savev(dest, "png", &[]).map_err(|e| e.to_string())
}

/// Copies the AppStream files of a package (`deb`) into the repository:
/// its metainfo, its icon in the three sizes and its screenshots. A package
/// with no metainfo is not an application, and nothing is done.
pub fn import_package(dir: &Path, deb: &str, package: &str) -> Result<(), String> {
    let listing = run("dpkg-deb", &["-c", deb], None, None, TIMEOUT)?;
    // dpkg-deb lists paths as "./usr/..." or, in the packages of Tauri, "usr/...".
    let has_metainfo = listing.stdout.lines().any(|l| {
        (l.contains("usr/share/metainfo/") || l.contains("usr/share/appdata/"))
            && (l.ends_with(".metainfo.xml") || l.ends_with(".appdata.xml"))
    });
    if !has_metainfo {
        return Ok(());
    }

    let work = std::env::temp_dir().join(format!("soplos-repo-manager-{}-{package}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let work_text = work.to_string_lossy().to_string();
        let x = run("dpkg-deb", &["-x", deb, &work_text], None, None, TIMEOUT)?;
        if !x.ok {
            return Err(x.error("dpkg-deb"));
        }
        let metainfo = ["usr/share/metainfo", "usr/share/appdata"]
            .iter()
            .filter_map(|d| fs::read_dir(work.join(d)).ok())
            .flat_map(|entries| entries.flatten().map(|e| e.path()))
            .find(|p| {
                let n = file_name(p);
                n.ends_with(".metainfo.xml") || n.ends_with(".appdata.xml")
            })
            .ok_or("no metainfo")?;
        let text = fs::read_to_string(&metainfo).map_err(|e| e.to_string())?;

        fs::create_dir_all(dir.join("appstream")).map_err(|e| e.to_string())?;
        fs::write(dir.join("appstream").join(format!("{package}.metainfo.xml")), &text).map_err(|e| e.to_string())?;

        // The media of an older version go: nothing old is left to be picked up.
        let media = dir.join("media").join(package);
        let _ = fs::remove_dir_all(&media);
        let icon = icon_name(&work, &text, package);
        let sources = icon_sources(&work, &icon);
        for size in ICON_SIZES {
            let out = media.join("icons").join(format!("{size}x{size}"));
            // The exact size when the package has it, else the smallest larger one.
            let source = sources
                .iter()
                .find(|(px, _)| *px == size)
                .or_else(|| sources.iter().filter(|(px, _)| *px >= size).last())
                .or_else(|| sources.first());
            if let Some((_, source)) = source {
                fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                write_icon(source, size, &out.join(format!("{icon}.png")))?;
            }
        }
        let mut shots = Vec::new();
        collect_screenshots(&work.join("usr/share"), &mut shots);
        if !shots.is_empty() {
            let out = media.join("screenshots");
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            for shot in shots {
                fs::copy(&shot, out.join(file_name(&shot))).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

/// Pictures whose name says they are a screenshot, as 2.x found them.
fn collect_screenshots(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_screenshots(&path, out);
        } else {
            let name = file_name(&path).to_lowercase();
            let picture = name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg");
            if picture && name.contains("screenshot") {
                out.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------- DEP-11 text

/// A YAML scalar written as a JSON string, which YAML reads as it is: no
/// quoting or escaping can go wrong.
fn q(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

fn lang(node: roxmltree::Node) -> String {
    node.attribute((XML_NS, "lang")).unwrap_or("C").to_string()
}

/// All the text inside a node, with its inline tags (code, em) as plain text.
fn inner_text(node: roxmltree::Node) -> String {
    let text: String = node.descendants().filter(|n| n.is_text()).map(|n| n.text().unwrap_or_default()).collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The text of the children named `tag`, by language.
fn by_lang(parent: roxmltree::Node, tag: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for n in parent.children().filter(|n| n.has_tag_name(tag)) {
        let text = inner_text(n);
        if !text.is_empty() {
            map.entry(lang(n)).or_insert(text);
        }
    }
    map
}

/// A description as HTML by language: every paragraph and every list, in
/// order, as the metainfo has them.
fn description(node: roxmltree::Node) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for child in node.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "p" => {
                let text = inner_text(child);
                if !text.is_empty() {
                    map.entry(lang(child)).or_default().push_str(&format!("<p>{}</p>", escape_html(&text)));
                }
            }
            tag @ ("ul" | "ol") => {
                let mut items: BTreeMap<String, String> = BTreeMap::new();
                for li in child.children().filter(|n| n.has_tag_name("li")) {
                    let text = inner_text(li);
                    if !text.is_empty() {
                        items.entry(lang(li)).or_default().push_str(&format!("<li>{}</li>", escape_html(&text)));
                    }
                }
                for (l, html) in items {
                    map.entry(l).or_default().push_str(&format!("<{tag}>{html}</{tag}>"));
                }
            }
            _ => {}
        }
    }
    map
}

fn write_lang_map(out: &mut String, key: &str, map: &BTreeMap<String, String>, indent: &str) {
    if map.is_empty() {
        return;
    }
    out.push_str(&format!("{indent}{key}:\n"));
    for (l, text) in map {
        out.push_str(&format!("{indent}  {l}: {}\n", q(text)));
    }
}

/// Seconds since 1970 of a date "YYYY-MM-DD".
fn unix_date(date: &str) -> Option<i64> {
    let mut parts = date.get(..10)?.split('-').map(|p| p.parse::<i64>());
    let (y, m, d) = (parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
    // Days from 1970-01-01 to the date (the civil calendar of Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) * 86400)
}

/// The width and height of a picture.
fn picture_size(path: &Path) -> Option<(i32, i32)> {
    gdk_pixbuf::Pixbuf::file_info(path).map(|(_, w, h)| (w, h))
}

/// One component of the DEP-11 file, from the metainfo of `package`, or None
/// when it lacks what a software center needs (a name, a summary, an icon).
fn component(dir: &Path, package: &str, metainfo: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(metainfo).ok()?;
    let root = doc.root_element();
    let child = |tag: &str| root.children().find(|n| n.has_tag_name(tag));
    let id = child("id").map(inner_text).filter(|s| !s.is_empty())?;
    let name = by_lang(root, "name");
    let summary = by_lang(root, "summary");
    if name.is_empty() || summary.is_empty() {
        return None;
    }

    // Icons: the ones copied from the package, in the sizes there are.
    let mut icons = Vec::new();
    for size in ICON_SIZES {
        let folder = dir.join("media").join(package).join("icons").join(format!("{size}x{size}"));
        if let Some(file) = fs::read_dir(&folder).ok().and_then(|mut e| e.find_map(|f| f.ok())).map(|f| file_name(&f.path())) {
            icons.push((size, file));
        }
    }
    if icons.is_empty() {
        return None;
    }

    let mut out = String::new();
    out.push_str(&format!("Type: {}\n", root.attribute("type").unwrap_or("generic")));
    out.push_str(&format!("ID: {}\n", q(&id)));
    out.push_str(&format!("Package: {}\n", q(package)));
    write_lang_map(&mut out, "Name", &name, "");
    write_lang_map(&mut out, "Summary", &summary, "");
    if let Some(d) = child("description") {
        write_lang_map(&mut out, "Description", &description(d), "");
    }
    if let Some(license) = child("project_license").map(inner_text).filter(|s| !s.is_empty()) {
        out.push_str(&format!("ProjectLicense: {}\n", q(&license)));
    }
    if let Some(dev) = child("developer") {
        let names = by_lang(dev, "name");
        if !names.is_empty() {
            out.push_str("Developer:\n");
            if let Some(dev_id) = dev.attribute("id") {
                out.push_str(&format!("  id: {}\n", q(dev_id)));
            }
            write_lang_map(&mut out, "name", &names, "  ");
        }
    } else {
        write_lang_map(&mut out, "DeveloperName", &by_lang(root, "developer_name"), "");
    }
    let urls: Vec<(String, String)> = root
        .children()
        .filter(|n| n.has_tag_name("url"))
        .filter_map(|n| Some((n.attribute("type").unwrap_or("homepage").to_string(), inner_text(n))).filter(|(_, u)| !u.is_empty()))
        .collect();
    if !urls.is_empty() {
        out.push_str("Url:\n");
        for (kind, url) in urls {
            out.push_str(&format!("  {kind}: {}\n", q(&url)));
        }
    }
    if let Some(cats) = child("categories") {
        let cats: Vec<String> = cats.children().filter(|n| n.has_tag_name("category")).map(inner_text).filter(|s| !s.is_empty()).collect();
        if !cats.is_empty() {
            out.push_str("Categories:\n");
            for c in cats {
                out.push_str(&format!("  - {}\n", q(&c)));
            }
        }
    }
    if let Some(kw) = child("keywords") {
        let mut words: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for k in kw.children().filter(|n| n.has_tag_name("keyword")) {
            let text = inner_text(k);
            if !text.is_empty() {
                words.entry(lang(k)).or_default().push(text);
            }
        }
        if !words.is_empty() {
            out.push_str("Keywords:\n");
            for (l, list) in words {
                out.push_str(&format!("  {l}:\n"));
                for w in list {
                    out.push_str(&format!("    - {}\n", q(&w)));
                }
            }
        }
    }
    out.push_str("Icon:\n  cached:\n");
    for (size, file) in &icons {
        out.push_str(&format!("    - name: {}\n      width: {size}\n      height: {size}\n", q(file)));
    }
    out.push_str("  remote:\n");
    for (size, file) in &icons {
        out.push_str(&format!("    - url: {}\n      width: {size}\n      height: {size}\n", q(&format!("{package}/icons/{size}x{size}/{file}"))));
    }
    let launch: Vec<String> = root
        .children()
        .filter(|n| n.has_tag_name("launchable") && n.attribute("type").unwrap_or("desktop-id") == "desktop-id")
        .map(inner_text)
        .filter(|s| !s.is_empty())
        .collect();
    if !launch.is_empty() {
        out.push_str("Launchable:\n  desktop-id:\n");
        for l in launch {
            out.push_str(&format!("    - {}\n", q(&l)));
        }
    }
    if let Some(p) = child("provides") {
        let bins: Vec<String> = p.children().filter(|n| n.has_tag_name("binary")).map(inner_text).filter(|s| !s.is_empty()).collect();
        if !bins.is_empty() {
            out.push_str("Provides:\n  binaries:\n");
            for b in bins {
                out.push_str(&format!("    - {}\n", q(&b)));
            }
        }
    }

    // Screenshots: only those whose picture is in the media of the package,
    // with the size the picture really has.
    if let Some(shots) = child("screenshots") {
        let mut text = String::new();
        let mut first = true;
        for shot in shots.children().filter(|n| n.has_tag_name("screenshot")) {
            let Some(image) = shot.children().find(|n| n.has_tag_name("image")).map(inner_text) else { continue };
            let base = image.rsplit('/').next().unwrap_or_default().to_string();
            let local = dir.join("media").join(package).join("screenshots").join(&base);
            let Some((w, h)) = picture_size(&local) else { continue };
            // The lines of one item of the list, each with its own indent:
            // the first opens the item with "- ", the rest line up under it.
            let mut lines: Vec<String> = Vec::new();
            if shot.attribute("type") == Some("default") || first {
                lines.push("default: true".to_string());
            }
            first = false;
            let captions = by_lang(shot, "caption");
            if !captions.is_empty() {
                lines.push("caption:".to_string());
                lines.extend(captions.iter().map(|(l, c)| format!("  {l}: {}", q(c))));
            }
            lines.push("source-image:".to_string());
            lines.push(format!("  url: {}", q(&format!("{package}/screenshots/{base}"))));
            lines.push(format!("  width: {w}"));
            lines.push(format!("  height: {h}"));
            for (i, line) in lines.iter().enumerate() {
                text.push_str(if i == 0 { "  - " } else { "    " });
                text.push_str(line);
                text.push('\n');
            }
        }
        if !text.is_empty() {
            out.push_str("Screenshots:\n");
            out.push_str(&text);
        }
    }

    if let Some(releases) = child("releases") {
        let mut text = String::new();
        for r in releases.children().filter(|n| n.has_tag_name("release")) {
            let Some(version) = r.attribute("version") else { continue };
            text.push_str(&format!("  - version: {}\n    type: {}\n", q(version), r.attribute("type").unwrap_or("stable")));
            if let Some(ts) = r.attribute("timestamp").and_then(|t| t.parse::<i64>().ok()).or_else(|| r.attribute("date").and_then(unix_date)) {
                text.push_str(&format!("    unix-timestamp: {ts}\n"));
            }
            if let Some(d) = r.children().find(|n| n.has_tag_name("description")) {
                write_lang_map(&mut text, "description", &description(d), "    ");
            }
        }
        if !text.is_empty() {
            out.push_str("Releases:\n");
            out.push_str(&text);
        }
    }
    if let Some(rating) = child("content_rating") {
        let attrs: Vec<(String, String)> = rating
            .children()
            .filter(|n| n.has_tag_name("content_attribute"))
            .filter_map(|n| Some((n.attribute("id")?.to_string(), inner_text(n))))
            .collect();
        let kind = rating.attribute("type").unwrap_or("oars-1.1");
        if attrs.is_empty() {
            out.push_str(&format!("ContentRating:\n  {kind}: {{}}\n"));
        } else {
            out.push_str(&format!("ContentRating:\n  {kind}:\n"));
            for (k, v) in attrs {
                out.push_str(&format!("    {k}: {}\n", q(&v)));
            }
        }
    }
    Some(out)
}

/// Where the media of the repository can be downloaded: the raw files of its
/// Git platform.
fn media_base_url(repo: &Repo) -> String {
    let url = repo.git_url.trim().trim_end_matches(".git").trim_end_matches('/');
    let branch = if repo.branch.trim().is_empty() { "main" } else { repo.branch.trim() };
    if let Some(path) = url.strip_prefix("https://github.com/") {
        format!("https://raw.githubusercontent.com/{path}/{branch}/media")
    } else if url.starts_with("https://") {
        // GitLab, Codeberg, Gitea and Forgejo serve raw files under the project.
        let raw = if url.contains("gitlab") { "-/raw" } else { "raw/branch" };
        format!("{url}/{raw}/{branch}/media")
    } else {
        String::new()
    }
}

/// Writes the DEP-11 files of the repository from its metainfo files, for the
/// packages it has now. Returns the paths of the files written, relative to
/// dists/<codename>, for the Release file.
pub fn regenerate(dir: &Path, repo: &Repo, packages: &[String]) -> Result<Vec<String>, String> {
    let layout = repos::layout(dir);
    let component_name = layout.components.first().cloned().unwrap_or_else(|| "main".to_string());
    let dep11 = dir.join("dists").join(&layout.codename).join(&component_name).join("dep11");
    fs::create_dir_all(&dep11).map_err(|e| e.to_string())?;

    let origin = super::repos::conf_value(dir, "Origin").unwrap_or_else(|| repo.name.clone());
    let time = run("date", &["-u", "+%Y%m%dT%H%M%S"], None, None, TIMEOUT).map(|r| r.stdout.trim().to_string()).unwrap_or_default();
    let mut text = format!("---\nFile: DEP-11\nVersion: '1.0'\nOrigin: {}\nMediaBaseUrl: {}\nTime: {time}\n", q(&origin), q(&media_base_url(repo)));
    let mut with_icons: Vec<String> = Vec::new();
    let mut names: Vec<&String> = packages.iter().collect();
    names.sort();
    names.dedup();
    for package in names {
        let Ok(metainfo) = fs::read_to_string(dir.join("appstream").join(format!("{package}.metainfo.xml"))) else { continue };
        if let Some(c) = component(dir, package, &metainfo) {
            text.push_str("---\n");
            text.push_str(&c);
            with_icons.push(package.clone());
        }
    }

    let mut written = Vec::new();
    for arch in &layout.architectures {
        let yml = dep11.join(format!("Components-{arch}.yml"));
        fs::write(&yml, &text).map_err(|e| e.to_string())?;
        let gz = run("gzip", &["-9", "-n", "-k", "-f", &yml.to_string_lossy()], None, None, TIMEOUT)?;
        if !gz.ok {
            return Err(gz.error("gzip"));
        }
        written.push(format!("{component_name}/dep11/Components-{arch}.yml"));
        written.push(format!("{component_name}/dep11/Components-{arch}.yml.gz"));
    }

    // The icons of every component, at the top of one tar per size.
    for size in ICON_SIZES {
        let tar = dep11.join(format!("icons-{size}x{size}.tar"));
        let mut args: Vec<String> = vec!["-cf".to_string(), tar.to_string_lossy().to_string()];
        for package in &with_icons {
            let folder = dir.join("media").join(package).join("icons").join(format!("{size}x{size}"));
            if let Ok(entries) = fs::read_dir(&folder) {
                for e in entries.flatten() {
                    args.extend(["-C".to_string(), folder.to_string_lossy().to_string(), file_name(&e.path())]);
                }
            }
        }
        if args.len() == 2 {
            // tar refuses to make an empty archive from no files.
            args.extend(["--files-from".to_string(), "/dev/null".to_string()]);
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let t = run("tar", &refs, None, None, TIMEOUT)?;
        if !t.ok {
            return Err(t.error("tar"));
        }
        written.push(format!("{component_name}/dep11/icons-{size}x{size}.tar"));
    }
    Ok(written)
}

/// Adds the DEP-11 files to the checksums of the Release file that reprepro
/// wrote (it does not list them), and signs Release and InRelease again.
pub fn sign_release(dir: &Path, files: &[String], fingerprint: &str) -> Result<(), String> {
    let layout = repos::layout(dir);
    let dist = dir.join("dists").join(&layout.codename);
    let release_path = dist.join("Release");
    let release = fs::read_to_string(&release_path).map_err(|e| e.to_string())?;

    let mut sums: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (section, tool) in [("MD5Sum:", "md5sum"), ("SHA1:", "sha1sum"), ("SHA256:", "sha256sum")] {
        for file in files {
            let path = dist.join(file);
            let size = fs::metadata(&path).map(|m| m.len()).map_err(|e| e.to_string())?;
            let r = run(tool, &[&path.to_string_lossy()], None, None, TIMEOUT)?;
            let hash = r.stdout.split_whitespace().next().unwrap_or_default().to_string();
            sums.entry(section).or_default().push(format!(" {hash} {size} {file}"));
        }
    }
    // Each line goes at the end of its section, after the lines of reprepro.
    let mut out: Vec<String> = Vec::new();
    let mut current: Option<&str> = None;
    for line in release.lines() {
        if !line.starts_with(' ') {
            if let Some(section) = current.take() {
                out.extend(sums.remove(section).unwrap_or_default());
            }
            current = sums.keys().copied().find(|s| line == *s);
        }
        let listed = files.iter().any(|f| line.ends_with(&format!(" {f}")));
        if !(line.starts_with(' ') && listed) {
            out.push(line.to_string());
        }
    }
    if let Some(section) = current {
        out.extend(sums.remove(section).unwrap_or_default());
    }
    let mut text = out.join("\n");
    text.push('\n');
    fs::write(&release_path, text).map_err(|e| e.to_string())?;

    // No --batch: gpg-agent must be free to ask for the passphrase of the key.
    let release_text = release_path.to_string_lossy().to_string();
    for (out_name, mode) in [("Release.gpg", "-abs"), ("InRelease", "--clearsign")] {
        let out_path = dist.join(out_name).to_string_lossy().to_string();
        let r = run("gpg", &["--yes", "--default-key", fingerprint, mode, "-o", &out_path, &release_text], None, None, Duration::from_secs(300))?;
        if !r.ok {
            return Err(r.error("gpg"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Only reads a repository (DEP11_REPO) and writes its components to DEP11_OUT,
    // so a real repository can be checked with appstreamcli without changing it.
    #[test]
    #[ignore]
    fn the_components_of_a_real_repository() {
        let dir = PathBuf::from(std::env::var("DEP11_REPO").unwrap());
        let repo = Repo { name: "test".into(), git_url: "https://github.com/SoplosLinux/tyron.git".into(), branch: "main".into(), ..Default::default() };
        let mut text = format!("---\nFile: DEP-11\nVersion: '1.0'\nOrigin: SoplosLinux\nMediaBaseUrl: {}\n", q(&media_base_url(&repo)));
        let mut count = 0;
        for entry in fs::read_dir(dir.join("appstream")).unwrap().flatten() {
            let package = file_name(&entry.path()).trim_end_matches(".metainfo.xml").to_string();
            let metainfo = fs::read_to_string(entry.path()).unwrap();
            if let Some(c) = component(&dir, &package, &metainfo) {
                text.push_str("---\n");
                text.push_str(&c);
                count += 1;
            }
        }
        fs::write(std::env::var("DEP11_OUT").unwrap(), text).unwrap();
        assert!(count > 0);
    }

    #[test]
    fn dates_of_releases_become_seconds() {
        assert_eq!(unix_date("1970-01-01"), Some(0));
        assert_eq!(unix_date("2026-11-16"), Some(1_794_787_200));
        assert_eq!(unix_date("bad"), None);
    }

    #[test]
    fn descriptions_keep_every_paragraph_and_list() {
        let xml = r#"<component><description><p>One</p><p xml:lang="es">Uno</p><ul><li>a</li><li>b &amp; c</li></ul><p>Two</p></description></component>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let d = description(doc.root_element().children().find(|n| n.has_tag_name("description")).unwrap());
        assert_eq!(d["C"], "<p>One</p><ul><li>a</li><li>b &amp; c</li></ul><p>Two</p>");
        assert_eq!(d["es"], "<p>Uno</p>");
    }

    #[test]
    fn the_media_of_a_repository_come_from_its_git_platform() {
        let mut repo = Repo { git_url: "https://github.com/SoplosLinux/tyron.git".into(), branch: "main".into(), ..Default::default() };
        assert_eq!(media_base_url(&repo), "https://raw.githubusercontent.com/SoplosLinux/tyron/main/media");
        repo.git_url = "https://gitlab.com/me/repo".into();
        assert_eq!(media_base_url(&repo), "https://gitlab.com/me/repo/-/raw/main/media");
    }
}
