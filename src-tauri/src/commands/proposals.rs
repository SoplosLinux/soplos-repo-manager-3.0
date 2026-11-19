// Team proposals and pull request workflow for Debian packages.
// Developers push package candidates to a proposal branch and open a PR on GitHub.
// The repository administrator can review, test and accept proposals, which imports
// packages into reprepro, signs them with the master GPG key, pushes to main,
// and closes the PR.

use super::credentials;
use super::publish;
use super::repos;
use super::run::{run, JobLogger};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;

const API_TIMEOUT: Duration = Duration::from_secs(30);
// The package itself travels here.
const PUSH_TIMEOUT: Duration = Duration::from_secs(1800);

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub user: String,
    pub user_avatar: String,
    pub branch: String,
    pub created_at: String,
    pub html_url: String,
    pub files: Vec<ProposalFile>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProposalFile {
    pub filename: String,
    pub raw_url: String,
    pub size: u64,
}

/// Parses the owner and repository name from a GitHub URL.
/// e.g. "https://github.com/SoplosLinux/soplos-kernels.git" -> Some(("SoplosLinux", "soplos-kernels"))
pub fn parse_github_owner_repo(git_url: &str) -> Option<(String, String)> {
    let url = git_url.trim().trim_end_matches(".git").trim_end_matches('/');
    let path = if let Some(p) = url.strip_prefix("https://github.com/") {
        p
    } else if let Some(p) = url.strip_prefix("git@github.com:") {
        p
    } else {
        return None;
    };
    let mut parts = path.split('/');
    let owner = parts.next()?.trim().to_string();
    let repo = parts.next()?.trim().to_string();
    if owner.is_empty() || repo.is_empty() {
        None
    } else {
        Some((owner, repo))
    }
}

// The headers, token included, go to curl on its standard input (-H @-), so
// the token never shows on a command line.
fn github_headers() -> (Vec<String>, String) {
    (vec!["-H".to_string(), "@-".to_string()], credentials::github_header_lines(credentials::active_token().as_deref()))
}

// ── GitHub API client via curl ────────────────────────────────────────────────

/// One request to the GitHub API. curl writes the HTTP status after the body
/// (`-w`), so an answer of GitHub that refuses the request (403, 404, 422...)
/// is an error with the reason GitHub gives, never taken as done. `body_file`
/// sends a body too large for a command line (a package in base64).
fn request(method: &str, endpoint: &str, body: Option<&serde_json::Value>, body_file: Option<&Path>, timeout: Duration) -> Result<serde_json::Value, String> {
    let (header_args, header_lines) = github_headers();
    let mut args = vec!["-sS".to_string(), "-X".to_string(), method.to_string(), "-w".to_string(), "\n%{http_code}".to_string()];
    args.extend(header_args);
    if body.is_some() || body_file.is_some() {
        args.push("-H".to_string());
        args.push("Content-Type: application/json".to_string());
    }
    if let Some(value) = body {
        args.push("-d".to_string());
        args.push(serde_json::to_string(value).map_err(|e| e.to_string())?);
    }
    if let Some(file) = body_file {
        args.push("--data-binary".to_string());
        args.push(format!("@{}", file.display()));
    }
    args.push(format!("https://api.github.com/{}", endpoint.trim_start_matches('/')));

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let res = run("curl", &arg_refs, None, Some(&header_lines), timeout)?;
    if !res.ok {
        return Err(res.error("GitHub API"));
    }
    let (text, status) = res.stdout.trim_end().rsplit_once('\n').unwrap_or(("", res.stdout.trim()));
    let status: u16 = status.trim().parse().unwrap_or(0);
    let value: serde_json::Value = if text.trim().is_empty() { serde_json::Value::Null } else { serde_json::from_str(text).map_err(|e| format!("Invalid answer from GitHub: {e}"))? };
    if !(200..300).contains(&status) {
        return Err(format!("GitHub ({status}): {}", github_error(&value)));
    }
    Ok(value)
}

fn api_get(endpoint: &str) -> Result<serde_json::Value, String> {
    request("GET", endpoint, None, None, API_TIMEOUT)
}

fn api_post(endpoint: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    request("POST", endpoint, Some(body), None, API_TIMEOUT)
}

fn api_patch(endpoint: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    request("PATCH", endpoint, Some(body), None, API_TIMEOUT)
}

fn api_delete(endpoint: &str) -> Result<(), String> {
    request("DELETE", endpoint, None, None, API_TIMEOUT).map(|_| ())
}

/// Sends a body too large for a command line, such as a package in base64: the
/// JSON goes to a private temporary file that curl reads, and is removed after.
fn api_post_file(endpoint: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let path = std::env::temp_dir().join(format!(
        "soplos-api-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::write(&path, serde_json::to_vec(body).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let res = request("POST", endpoint, None, Some(&path), PUSH_TIMEOUT);
    let _ = fs::remove_file(&path);
    res
}

/// Every page of a list of the API (100 items a page, as many pages as there are).
fn api_get_all(endpoint: &str) -> Result<Vec<serde_json::Value>, String> {
    let join = if endpoint.contains('?') { '&' } else { '?' };
    let mut all = Vec::new();
    for page in 1..=50 {
        let items = api_get(&format!("{endpoint}{join}per_page=100&page={page}"))?;
        let items = items.as_array().cloned().unwrap_or_default();
        let last = items.len() < 100;
        all.extend(items);
        if last {
            break;
        }
    }
    Ok(all)
}

/// Uploads a package as a blob of the repository and returns its SHA.
fn upload_blob(owner: &str, repo: &str, file: &Path) -> Result<String, String> {
    let bytes = fs::read(file).map_err(|e| format!("{} could not be read: {e}", file.display()))?;
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let body = serde_json::json!({ "content": encoded, "encoding": "base64" });
    let res = api_post_file(&format!("repos/{owner}/{repo}/git/blobs"), &body)?;
    res.get("sha")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("GitHub refused the package {}: {}", file.display(), github_error(&res)))
}

/// Packages above this size do not go through the blobs API: in base64 inside
/// a JSON body they pass what GitHub takes in one request, and it answers 422
/// "your input was too large to process" (a kernel of 92 MB did). They go up
/// with git instead, which sends the file as it is.
const API_BLOB_LIMIT: u64 = 25 * 1024 * 1024;

/// GitHub refuses any file above 100 MiB, in a push or through the API.
const GITHUB_FILE_LIMIT: u64 = 100 * 1024 * 1024;

/// Uploads large packages with git and returns the SHA of each, in order, and
/// the branch that carries them. The commit holds only those files and has no
/// parent, so git sends them and nothing of the repository is downloaded,
/// which is what made git unusable for proposals before. The proposal is then
/// made with the API over those blobs, and the carrier branch is deleted: the
/// blobs stay, because the branch of the proposal uses them.
fn upload_with_git(logger: &JobLogger, owner: &str, repo: &str, files: &[&Path], stamp: u64) -> Result<(Vec<String>, String), String> {
    let work = std::env::temp_dir().join(format!("soplos-proposal-{}-{stamp}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(Vec<String>, String), String> {
        publish::git_logged_ok(Some(logger), &work, &["init", "-q"], API_TIMEOUT)?;
        let mut shas = Vec::with_capacity(files.len());
        for file in files {
            let path = file.to_string_lossy().to_string();
            let name = file.file_name().map(|n| n.to_string_lossy().to_string()).ok_or("A package has no name")?;
            let sha = publish::git_logged_ok(Some(logger), &work, &["hash-object", "-w", "--", &path], PUSH_TIMEOUT)?.trim().to_string();
            publish::git_logged_ok(Some(logger), &work, &["update-index", "--add", "--cacheinfo", &format!("100644,{sha},{name}")], API_TIMEOUT)?;
            shas.push(sha);
        }
        let tree = publish::git_logged_ok(Some(logger), &work, &["write-tree"], API_TIMEOUT)?.trim().to_string();
        let commit = publish::git_logged_ok(
            Some(logger),
            &work,
            &["-c", "user.name=Soplos Repo Manager", "-c", "user.email=repo-manager@soplos.org", "commit-tree", &tree, "-m", "Packages of a proposal"],
            API_TIMEOUT,
        )?
        .trim()
        .to_string();
        let carrier = format!("proposal-upload/{stamp}");
        let url = format!("https://github.com/{owner}/{repo}.git");
        publish::git_logged_ok(Some(logger), &work, &["push", &url, &format!("{commit}:refs/heads/{carrier}")], PUSH_TIMEOUT)?;
        Ok((shas, carrier))
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

/// Writes the contents of a file of the repository in `dest`. The raw media
/// type makes curl receive the bytes of the package, not its base64 form.
fn api_download(endpoint: &str, dest: &Path) -> Result<(), String> {
    let (header_args, header_lines) = github_headers();
    let mut args = vec![
        "-sSL".to_string(),
        "--fail".to_string(),
        "-H".to_string(),
        "Accept: application/vnd.github.raw".to_string(),
        "-o".to_string(),
        dest.to_string_lossy().to_string(),
    ];
    args.extend(header_args);
    args.push(format!("https://api.github.com/{}", endpoint.trim_start_matches('/')));

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let res = run("curl", &arg_refs, None, Some(&header_lines), PUSH_TIMEOUT)?;
    if !res.ok {
        return Err(format!("The package could not be downloaded: {}", res.error("GitHub")));
    }
    Ok(())
}

/// The reason GitHub gives for refusing a request, with the detail of a
/// validation failure when there is one.
fn github_error(value: &serde_json::Value) -> String {
    let mut text = value
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("no reason given")
        .to_string();
    if let Some(errors) = value.get("errors").and_then(|e| e.as_array()) {
        let detail: Vec<String> = errors
            .iter()
            .filter_map(|e| {
                e.get("message")
                    .and_then(|m| m.as_str())
                    .map(|m| m.to_string())
                    .or_else(|| e.get("field").and_then(|f| f.as_str()).map(|f| format!("{f} is not valid")))
                    .or_else(|| e.get("code").and_then(|c| c.as_str()).map(|c| c.to_string()))
            })
            .collect();
        if !detail.is_empty() {
            text.push_str(&format!(" ({})", detail.join("; ")));
        }
    }
    text
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// The open pull requests of a repository on GitHub, every page of them.
pub(crate) fn open_pull_requests(owner: &str, repo: &str) -> Result<Vec<serde_json::Value>, String> {
    api_get_all(&format!("repos/{owner}/{repo}/pulls?state=open"))
}

/// Lists open proposals (Pull Requests) on the repository's GitHub remote.
#[tauri::command(async)]
pub fn list_proposals(name: String) -> Result<Vec<Proposal>, String> {
    let repo = repos::by_name(&name)?;
    let (owner, repo_name) = parse_github_owner_repo(&repo.git_url)
        .ok_or_else(|| "The repository has no GitHub URL".to_string())?;

    let prs = api_get(&format!("repos/{owner}/{repo_name}/pulls?state=open&sort=created&direction=desc"))?;
    let Some(items) = prs.as_array() else {
        return Ok(Vec::new());
    };

    let mut proposals = Vec::new();
    for pr in items {
        let number = pr.get("number").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = pr.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let body = pr.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let html_url = pr.get("html_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let created_at = pr.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let user = pr.get("user").and_then(|u| u.get("login")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let user_avatar = pr.get("user").and_then(|u| u.get("avatar_url")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let branch = pr.get("head").and_then(|h| h.get("ref")).and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Query PR files
        let mut files = Vec::new();
        if let Ok(file_items) = api_get_all(&format!("repos/{owner}/{repo_name}/pulls/{number}/files")) {
            {
                for f in &file_items {
                    let filename = f.get("filename").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let raw_url = f.get("raw_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if filename.ends_with(".deb") {
                        files.push(ProposalFile {
                            filename,
                            raw_url,
                            size: 0,
                        });
                    }
                }
            }
        }

        proposals.push(Proposal {
            number,
            title,
            body,
            user,
            user_avatar,
            branch,
            created_at,
            html_url,
            files,
        });
    }

    Ok(proposals)
}

/// Creates a new proposal with the GitHub API: the .deb files go up as blobs, a
/// commit is made over the base branch and a Pull Request is opened from it.
/// Nothing of the repository is downloaded, git is not used and no local
/// reprepro installation or GPG key is involved.
#[tauri::command(async)]
pub fn send_proposal(
    app: tauri::AppHandle,
    target: String,
    title: String,
    description: String,
    paths: Vec<String>,
    // The closing line of the pull request, in the language of the interface.
    footer: String,
) -> Result<u64, String> {
    if paths.is_empty() {
        return Err("No .deb packages were chosen".to_string());
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("The proposal needs a title".to_string());
    }

    if credentials::active_token().is_none() {
        return Err("Add a GitHub account in Accounts before sending packages".to_string());
    }

    // Resolve target: check official/custom targets, or local repos, or raw git URL
    let (display_name, git_url, base_branch) = if let Some(t) = super::targets::find_target(&target) {
        (t.name, t.git_url, t.branch)
    } else if let Ok(r) = repos::by_name(&target) {
        (r.name, r.git_url, if r.branch.trim().is_empty() { "main".to_string() } else { r.branch })
    } else if target.starts_with("https://") || target.starts_with("git@") {
        (target.clone(), target.clone(), "main".to_string())
    } else {
        return Err(format!("Unknown destination: {target}"));
    };

    let (owner, repo_name) = parse_github_owner_repo(&git_url)
        .ok_or_else(|| format!("The destination '{git_url}' has no valid GitHub URL"))?;

    let logger = JobLogger::new_step(app, "sendTitle", &[("name", &display_name)]);
    logger.step("sendStart", &[("title", &title)]);

    // Unique proposal branch name
    let sanitized_title: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let sanitized_title = sanitized_title.trim_matches('-');
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let proposal_branch = format!("proposal/{}-{}", sanitized_title, timestamp);

    // The whole proposal is made with the API: the packages go up as blobs, the
    // branch is a tree over the one of the base branch and the commit is made
    // there. Nothing of the repository is downloaded and git is not used, so a
    // destination that holds gigabytes of packages costs the package itself.
    // The branch that carried the large packages, deleted once the proposal
    // has them or when it fails.
    let carrier: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
    let run_send = || -> Result<u64, String> {
        logger.step("fetchBase", &[("branch", &base_branch)]);
        let base_ref = api_get(&format!("repos/{owner}/{repo_name}/git/ref/heads/{base_branch}"))?;
        let base_sha = base_ref
            .get("object")
            .and_then(|o| o.get("sha"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("The branch {base_branch} could not be read: {}", github_error(&base_ref)))?
            .to_string();

        let base_commit = api_get(&format!("repos/{owner}/{repo_name}/git/commits/{base_sha}"))?;
        let base_tree = base_commit
            .get("tree")
            .and_then(|t| t.get("sha"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("The tree of {base_branch} could not be read: {}", github_error(&base_commit)))?
            .to_string();

        // What GitHub would refuse anyway stops here, before anything goes up.
        let mut small: Vec<&Path> = Vec::new();
        let mut large: Vec<&Path> = Vec::new();
        for p in &paths {
            let src = Path::new(p);
            let size = fs::metadata(src).map_err(|e| format!("{} could not be read: {e}", src.display()))?.len();
            if size > GITHUB_FILE_LIMIT {
                let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                return Err(format!(
                    "{name} weighs {:.1} MB: GitHub does not accept files above 100 MB",
                    size as f64 / 1024.0 / 1024.0
                ));
            }
            if size > API_BLOB_LIMIT {
                large.push(src);
            } else {
                small.push(src);
            }
        }

        let mut entries = Vec::new();
        let entry = |fname: &str, sha: &str| {
            serde_json::json!({
                "path": format!("staging/{fname}"),
                "mode": "100644",
                "type": "blob",
                "sha": sha
            })
        };
        for src in &small {
            let Some(fname) = src.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
            logger.step("addPackage", &[("file", &fname)]);
            let sha = upload_blob(&owner, &repo_name, src)?;
            entries.push(entry(&fname, &sha));
        }
        if !large.is_empty() {
            for src in &large {
                let fname = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                logger.step("addPackage", &[("file", &fname)]);
            }
            logger.step("uploadLarge", &[("count", &large.len().to_string())]);
            let (shas, branch) = upload_with_git(&logger, &owner, &repo_name, &large, timestamp)?;
            *carrier.borrow_mut() = Some(branch);
            for (src, sha) in large.iter().zip(shas.iter()) {
                let fname = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                entries.push(entry(&fname, sha));
            }
        }

        logger.step("commitProposal", &[]);
        let tree_body = serde_json::json!({ "base_tree": base_tree, "tree": entries });
        let tree = api_post(&format!("repos/{owner}/{repo_name}/git/trees"), &tree_body)?;
        let tree_sha = tree
            .get("sha")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("The tree of the proposal could not be made: {}", github_error(&tree)))?
            .to_string();

        let commit_body = serde_json::json!({
            "message": format!("Proposal: {}\n\n{}", title, description),
            "tree": tree_sha,
            "parents": [base_sha]
        });
        let commit = api_post(&format!("repos/{owner}/{repo_name}/git/commits"), &commit_body)?;
        let commit_sha = commit
            .get("sha")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("The commit of the proposal could not be made: {}", github_error(&commit)))?
            .to_string();

        logger.step("createBranch", &[("branch", &proposal_branch)]);
        let ref_body = serde_json::json!({
            "ref": format!("refs/heads/{proposal_branch}"),
            "sha": commit_sha
        });
        let created = api_post(&format!("repos/{owner}/{repo_name}/git/refs"), &ref_body)?;
        if created.get("ref").is_none() {
            return Err(format!("The branch of the proposal could not be made: {}", github_error(&created)));
        }

        // Call GitHub API to create Pull Request
        logger.step("openPr", &[]);
        let pr_body = serde_json::json!({
            "title": title,
            "body": format!("{}\n\n---\n*{}*", description, footer.trim()),
            "head": proposal_branch.clone(),
            "base": base_branch
        });

        let res = api_post(&format!("repos/{owner}/{repo_name}/pulls"), &pr_body);
        let pr_number = match res.as_ref().ok().and_then(|r| r.get("number")).and_then(|v| v.as_u64()) {
            Some(number) => number,
            None => {
                // The branch may already carry its proposal: GitHub then
                // refuses a second one, and the number of the first is the
                // answer the user asked for.
                let existing = api_get(&format!(
                    "repos/{owner}/{repo_name}/pulls?state=open&head={owner}:{proposal_branch}"
                ))
                .ok()
                .and_then(|list| {
                    list.as_array()
                        .and_then(|items| items.first())
                        .and_then(|pr| pr.get("number"))
                        .and_then(|v| v.as_u64())
                });
                match existing {
                    Some(number) => number,
                    None => return Err(format!("GitHub refused the pull request: {}", res.err().unwrap_or_default())),
                }
            }
        };

        logger.step("prCreated", &[("number", &pr_number.to_string())]);
        Ok(pr_number)
    };

    let result = run_send();
    if let Some(branch) = carrier.borrow().as_ref() {
        let _ = api_delete(&format!("repos/{owner}/{repo_name}/git/refs/heads/{branch}"));
    }

    match result {
        Ok(pr_number) => {
            logger.done(true);
            Ok(pr_number)
        }
        Err(err) => {
            logger.step("sendFailed", &[("error", &err.to_string())]);
            logger.done(false);
            Err(err)
        }
    }
}

/// Accepts a proposal: its packages are downloaded from the API, checked,
/// added with reprepro, the indexes and the data of the software centers are
/// made and signed, the repository is published, and the pull request is
/// commented, closed and its branch deleted.
///
/// Nothing is published unless every package of the proposal was added. With
/// `clean_first` the packages the repository had are moved aside (on the same
/// disk, so it costs nothing) and put back if anything fails, so a refused
/// package never leaves the repository empty.
#[tauri::command(async)]
pub fn accept_proposal(
    app: tauri::AppHandle,
    name: String,
    pr_number: u64,
    clean_first: bool,
    // The comment left on the pull request, in the language of the interface.
    comment: String,
) -> Result<(), String> {
    let repo = repos::by_name(&name)?;
    let (owner, repo_name) = parse_github_owner_repo(&repo.git_url)
        .ok_or_else(|| "The repository has no valid GitHub URL".to_string())?;
    let logger = JobLogger::new_step(app.clone(), "acceptTitle", &[("number", &pr_number.to_string()), ("repo", &repo.name)]);
    let dir = Path::new(&repo.dir).to_path_buf();
    let staging_dir = dir.join("staging");
    // Next to the repository, not inside it: publishing adds everything inside.
    let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let aside = dir.with_file_name(format!(".{folder}-before-proposal"));

    let work = || -> Result<(), String> {
        logger.step("readPr", &[("number", &pr_number.to_string())]);
        let pr = api_get(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}"))?;
        let branch = pr
            .get("head")
            .and_then(|h| h.get("ref"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| "The branch of the pull request is unknown".to_string())?
            .to_string();
        // The commit of the proposal, also when it comes from a fork.
        let head_sha = pr.get("head").and_then(|h| h.get("sha")).and_then(|v| v.as_str()).unwrap_or(&branch).to_string();

        // Only the packages the proposal adds, from the API, every page of them.
        logger.step("extract", &[]);
        let _ = fs::remove_dir_all(&staging_dir);
        fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;
        let mut deb_paths = Vec::new();
        for file in api_get_all(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}/files"))? {
            let filename = file.get("filename").and_then(|v| v.as_str()).unwrap_or("");
            if !filename.ends_with(".deb") || file.get("status").and_then(|s| s.as_str()) == Some("removed") {
                continue;
            }
            let Some(short) = Path::new(filename).file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
            let dest = staging_dir.join(&short);
            logger.step("downloadPackage", &[("file", &short)]);
            api_download(&format!("repos/{owner}/{repo_name}/contents/{filename}?ref={head_sha}"), &dest)?;
            deb_paths.push(dest);
        }
        if deb_paths.is_empty() {
            return Err("The branch of the proposal has no .deb files".to_string());
        }

        // Each package is looked at before the repository is touched: what
        // reprepro would refuse stops here, with the repository as it was.
        let layout = repos::layout(&dir);
        for path in &deb_paths {
            let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let fields = run("dpkg-deb", &["-f", &path.to_string_lossy(), "Package", "Version", "Architecture", "Section", "Priority"], None, None, API_TIMEOUT)?;
            let get = |k: &str| fields.stdout.lines().find_map(|l| l.strip_prefix(&format!("{k}: "))).unwrap_or("").trim().to_string();
            if !fields.ok || get("Package").is_empty() || get("Version").is_empty() {
                return Err(format!("{file} is not a valid Debian package"));
            }
            if get("Section").is_empty() || get("Priority").is_empty() {
                return Err(format!("{file} has no Section or Priority: reprepro would skip it"));
            }
            let arch = get("Architecture");
            if arch != "all" && !layout.architectures.contains(&arch) {
                return Err(format!("{file} is for {arch}, which the repository does not have"));
            }
        }

        if clean_first {
            logger.step("preClean", &[]);
            set_aside(&dir, &aside)?;
            publish::wipe_packages_and_reinit(&dir)?;
        }

        logger.step("include", &[("count", &deb_paths.len().to_string())]);
        let dir_text = dir.to_string_lossy().to_string();
        for path in &deb_paths {
            let path_text = path.to_string_lossy().to_string();
            let args = ["-b", &dir_text, "--ignore=unknownfield", "--export=silent-never", "includedeb", &layout.codename, &path_text];
            let r = run("reprepro", &args, None, None, PUSH_TIMEOUT)?;
            if !r.ok {
                let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                return Err(format!("reprepro refused {file}: {}", r.error("reprepro")));
            }
            let package = run("dpkg-deb", &["-f", &path_text, "Package"], None, None, API_TIMEOUT).map(|r| r.stdout.trim().to_string()).unwrap_or_default();
            if !package.is_empty() {
                let _ = super::appstream::import_package(&dir, &path_text, &package);
            }
        }
        let _ = fs::remove_dir_all(&staging_dir);

        // The indexes, the data of the software centers and the Release, signed.
        logger.step("signIndexes", &[]);
        super::packages::export_with_store(&dir, &repo)?;

        logger.step("pushMain", &[]);
        publish::publish(Some(&logger), &repo, &format!("Accept proposal #{pr_number}: {}\n\nBranch: {branch}", repo.name))?;
        // Published: what was set aside is no longer needed.
        let _ = fs::remove_dir_all(&aside);

        // The pull request: comment, close, branch deleted. The packages are
        // already published, so a refusal here is reported but not undone.
        logger.step("closePr", &[("number", &pr_number.to_string())]);
        let mut problems = Vec::new();
        if let Err(e) = api_post(&format!("repos/{owner}/{repo_name}/issues/{pr_number}/comments"), &serde_json::json!({ "body": comment.trim() })) {
            problems.push(e);
        }
        if let Err(e) = api_patch(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}"), &serde_json::json!({ "state": "closed" })) {
            problems.push(e);
        }
        logger.step("deleteBranch", &[("branch", &branch)]);
        if let Err(e) = api_delete(&format!("repos/{owner}/{repo_name}/git/refs/heads/{branch}")) {
            problems.push(e);
        }
        if !problems.is_empty() {
            return Err(format!("The packages are published, but the pull request could not be closed: {}", problems.join("; ")));
        }
        Ok(())
    };

    let result = work();
    let _ = fs::remove_dir_all(&staging_dir);
    match result {
        Ok(()) => {
            logger.step("accepted", &[]);
            logger.done(true);
            Ok(())
        }
        Err(e) => {
            // Anything that fails before publishing leaves the repository as it was.
            if aside.exists() {
                restore_aside(&dir, &aside);
                let _ = repos::export_signed(&dir);
            }
            logger.step("error", &[("error", &e)]);
            logger.done(false);
            Err(e)
        }
    }
}

/// The folders of the packages and their indexes, moved next to the
/// repository (a rename: no copy, whatever their size).
const PACKAGE_DIRS: [&str; 5] = ["pool", "db", "dists", "media", "appstream"];

fn set_aside(dir: &Path, aside: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(aside);
    fs::create_dir_all(aside).map_err(|e| e.to_string())?;
    for d in PACKAGE_DIRS {
        let from = dir.join(d);
        if from.exists() {
            fs::rename(&from, aside.join(d)).map_err(|e| format!("{d} could not be set aside: {e}"))?;
        }
    }
    Ok(())
}

fn restore_aside(dir: &Path, aside: &Path) {
    for d in PACKAGE_DIRS {
        let kept = aside.join(d);
        if kept.exists() {
            let _ = fs::remove_dir_all(dir.join(d));
            let _ = fs::rename(&kept, dir.join(d));
        }
    }
    let _ = fs::remove_dir_all(aside);
}

/// Rejects a proposal: the comment with the reason, the pull request closed
/// and its branch deleted. Every refusal of GitHub is reported.
#[tauri::command(async)]
pub fn reject_proposal(name: String, pr_number: u64, comment: String) -> Result<(), String> {
    let repo = repos::by_name(&name)?;
    let (owner, repo_name) = parse_github_owner_repo(&repo.git_url)
        .ok_or_else(|| "The repository has no valid GitHub URL".to_string())?;

    let pr = api_get(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}"))?;
    // Written by the interface, in its language, with the reason if there is one.
    api_post(&format!("repos/{owner}/{repo_name}/issues/{pr_number}/comments"), &serde_json::json!({ "body": comment.trim() }))?;
    api_patch(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}"), &serde_json::json!({ "state": "closed" }))?;

    // The branch of the proposal, when it is in this repository (not a fork).
    let same_repo = pr.get("head").and_then(|h| h.get("repo")).and_then(|r| r.get("full_name")).and_then(|v| v.as_str())
        == Some(&format!("{owner}/{repo_name}"));
    if let (true, Some(branch)) = (same_repo, pr.get("head").and_then(|h| h.get("ref")).and_then(|v| v.as_str())) {
        api_delete(&format!("repos/{owner}/{repo_name}/git/refs/heads/{branch}"))?;
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_proposal_puts_the_packages_back() {
        let base = std::env::temp_dir().join(format!("soplos-aside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let dir = base.join("repo");
        let aside = base.join(".repo-before-proposal");
        fs::create_dir_all(dir.join("pool/main/k")).unwrap();
        fs::create_dir_all(dir.join("conf")).unwrap();
        fs::write(dir.join("pool/main/k/kernel.deb"), "old").unwrap();
        fs::write(dir.join("conf/distributions"), "Codename: stable").unwrap();

        set_aside(&dir, &aside).unwrap();
        assert!(!dir.join("pool").exists());
        assert!(dir.join("conf/distributions").exists(), "the configuration stays");
        fs::create_dir_all(dir.join("pool/main/n")).unwrap();
        fs::write(dir.join("pool/main/n/new.deb"), "new").unwrap();

        restore_aside(&dir, &aside);
        assert_eq!(fs::read_to_string(dir.join("pool/main/k/kernel.deb")).unwrap(), "old");
        assert!(!dir.join("pool/main/n").exists());
        assert!(!aside.exists());
        fs::remove_dir_all(base).unwrap();
    }

    // Talks to GitHub (only reads): run with --ignored.
    #[test]
    #[ignore]
    fn a_refusal_of_github_is_an_error() {
        let e = api_get("repos/SoplosLinux/this-repository-does-not-exist").unwrap_err();
        assert!(e.starts_with("GitHub (404)"), "{e}");
        assert!(api_get("repos/SoplosLinux/soplos-kernels").is_ok());
    }

    #[test]
    fn reads_the_owner_and_the_repository() {
        assert_eq!(parse_github_owner_repo("https://github.com/SoplosLinux/soplos-kernels.git"), Some(("SoplosLinux".into(), "soplos-kernels".into())));
        assert_eq!(parse_github_owner_repo("git@github.com:SoplosLinux/tyron.git"), Some(("SoplosLinux".into(), "tyron".into())));
        assert_eq!(parse_github_owner_repo("https://gitlab.com/x/y"), None);
    }
}
