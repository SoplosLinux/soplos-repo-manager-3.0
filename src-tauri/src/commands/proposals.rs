// Team proposals and pull request workflow for Debian packages.
// Developers push package candidates to a proposal branch and open a PR on GitHub.
// The repository administrator can review, test and accept proposals, which imports
// packages into reprepro, signs them with the master GPG key, pushes to main,
// and closes the PR.

use super::credentials;
use super::publish::{self, git_logged, git_logged_ok};
use super::repos;
use super::run::{run, JobLogger};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;

const API_TIMEOUT: Duration = Duration::from_secs(30);
// Downloading or uploading a repository of kernels can take long: a shallow
// fetch of the base branch is still a whole copy of its current files.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(3600);

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

fn api_get(endpoint: &str) -> Result<serde_json::Value, String> {
    let mut args = vec!["-sS".to_string(), "-X".to_string(), "GET".to_string()];
    let (header_args, header_lines) = github_headers();
    args.extend(header_args);
    args.push(format!("https://api.github.com/{}", endpoint.trim_start_matches('/')));

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let res = run("curl", &arg_refs, None, Some(&header_lines), API_TIMEOUT)?;
    if !res.ok {
        return Err(res.error("GitHub API"));
    }
    serde_json::from_str(&res.stdout).map_err(|e| format!("Invalid answer from GitHub: {e}"))
}

fn api_post(endpoint: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut args = vec![
        "-sS".to_string(),
        "-X".to_string(),
        "POST".to_string(),
        "-H".to_string(),
        "Content-Type: application/json".to_string(),
    ];
    let (header_args, header_lines) = github_headers();
    args.extend(header_args);
    let payload = serde_json::to_string(body).map_err(|e| e.to_string())?;
    args.push("-d".to_string());
    args.push(payload);
    args.push(format!("https://api.github.com/{}", endpoint.trim_start_matches('/')));

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let res = run("curl", &arg_refs, None, Some(&header_lines), API_TIMEOUT)?;
    if !res.ok {
        return Err(res.error("GitHub API"));
    }
    serde_json::from_str(&res.stdout).map_err(|e| format!("Invalid answer from GitHub: {e}"))
}

fn api_patch(endpoint: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut args = vec![
        "-sS".to_string(),
        "-X".to_string(),
        "PATCH".to_string(),
        "-H".to_string(),
        "Content-Type: application/json".to_string(),
    ];
    let (header_args, header_lines) = github_headers();
    args.extend(header_args);
    let payload = serde_json::to_string(body).map_err(|e| e.to_string())?;
    args.push("-d".to_string());
    args.push(payload);
    args.push(format!("https://api.github.com/{}", endpoint.trim_start_matches('/')));

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let res = run("curl", &arg_refs, None, Some(&header_lines), API_TIMEOUT)?;
    if !res.ok {
        return Err(res.error("GitHub API"));
    }
    serde_json::from_str(&res.stdout).map_err(|e| format!("Invalid answer from GitHub: {e}"))
}

// ── Commands ──────────────────────────────────────────────────────────────────

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
        if let Ok(files_json) = api_get(&format!("repos/{owner}/{repo_name}/pulls/{number}/files")) {
            if let Some(file_items) = files_json.as_array() {
                for f in file_items {
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

/// Creates a new proposal: copies the specified .deb files into an isolated branch,
/// pushes the branch to GitHub, and creates a Pull Request against the base branch.
/// Works completely independently of local reprepro installations and GPG keys.
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

    // Create an isolated temporary workspace in std::env::temp_dir()
    let tmp_dir = std::env::temp_dir().join(format!("soplos-send-{}-{}", std::process::id(), timestamp));
    let _ = fs::remove_dir_all(&tmp_dir);
    fs::create_dir_all(&tmp_dir).map_err(|e| format!("The temporary folder could not be made: {e}"))?;

    let (author_name, author_email) = if let Ok(u) = credentials::get_github_user() {
        let name = u.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| u.login.clone());
        let email = format!("{}@users.noreply.github.com", u.login);
        (name, email)
    } else {
        ("Contributor".to_string(), "contributor@users.noreply.github.com".to_string())
    };

    let run_send = || -> Result<u64, String> {
        logger.step("workspace", &[("name", &display_name)]);
        git_logged_ok(Some(&logger), &tmp_dir, &["init"], API_TIMEOUT)?;
        git_logged_ok(Some(&logger), &tmp_dir, &["config", "user.name", &author_name], API_TIMEOUT)?;
        git_logged_ok(Some(&logger), &tmp_dir, &["config", "user.email", &author_email], API_TIMEOUT)?;

        let clean = credentials::clean_url(&git_url);
        git_logged_ok(Some(&logger), &tmp_dir, &["remote", "add", "origin", &clean], API_TIMEOUT)?;

        logger.step("fetchBase", &[("branch", &base_branch)]);
        git_logged_ok(Some(&logger), &tmp_dir, &["fetch", "--depth", "1", "origin", &base_branch], NETWORK_TIMEOUT)?;

        logger.step("createBranch", &[("branch", &proposal_branch)]);
        git_logged_ok(Some(&logger), &tmp_dir, &["checkout", "-b", &proposal_branch, &format!("origin/{base_branch}")], NETWORK_TIMEOUT)?;

        // Copy packages into a staging/ folder on the branch
        let staging_dir = tmp_dir.join("staging");
        fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;

        for p in &paths {
            let src = Path::new(p);
            if let Some(fname) = src.file_name() {
                let dst = staging_dir.join(fname);
                logger.step("addPackage", &[("file", &fname.to_string_lossy())]);
                fs::copy(src, &dst).map_err(|e| format!("{} could not be copied: {}", p, e))?;
            }
        }

        // Git add, commit, push
        logger.step("commitProposal", &[]);
        git_logged_ok(Some(&logger), &tmp_dir, &["add", "-f", "staging/"], NETWORK_TIMEOUT)?;
        git_logged_ok(
            Some(&logger),
            &tmp_dir,
            &["commit", "-m", &format!("Proposal: {}\n\n{}", title, description)],
            API_TIMEOUT,
        )?;

        logger.step("pushBranch", &[("branch", &proposal_branch)]);
        git_logged_ok(Some(&logger), &tmp_dir, &["push", "-u", "origin", &proposal_branch], NETWORK_TIMEOUT)?;

        // Call GitHub API to create Pull Request
        logger.step("openPr", &[]);
        let pr_body = serde_json::json!({
            "title": title,
            "body": format!("{}\n\n---\n*{}*", description, footer.trim()),
            "head": proposal_branch,
            "base": base_branch
        });

        let res = api_post(&format!("repos/{owner}/{repo_name}/pulls"), &pr_body)?;
        let pr_number = res.get("number").and_then(|v| v.as_u64()).ok_or_else(|| {
            let msg = res.get("message").and_then(|m| m.as_str()).unwrap_or("The pull request could not be made");
            format!("Error de GitHub: {msg}")
        })?;

        logger.step("prCreated", &[("number", &pr_number.to_string())]);
        Ok(pr_number)
    };

    let result = run_send();
    let _ = fs::remove_dir_all(&tmp_dir);

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

/// Accepts a proposal: downloads/extracts packages from the proposal branch,
/// processes them with reprepro, signs them with GPG, pushes to main, closes the PR,
/// and deletes the remote branch.
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
    logger.step("readPr", &[("number", &pr_number.to_string())]);

    let pr = api_get(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}"))?;
    let branch = pr.get("head").and_then(|h| h.get("ref")).and_then(|v| v.as_str()).ok_or_else(|| {
        "The branch of the pull request is unknown".to_string()
    })?.to_string();

    let dir = Path::new(&repo.dir);

    // Fetch the proposal branch
    logger.step("fetchBranch", &[("branch", &branch)]);
    git_logged_ok(Some(&logger), dir, &["fetch", "origin", &branch], NETWORK_TIMEOUT)?;

    // Checkout the staging/ directory directly from origin/<branch>
    logger.step("extract", &[]);
    let staging_dir = dir.join("staging");
    let _ = fs::remove_dir_all(&staging_dir);
    fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;

    let checkout_res = git_logged(
        Some(&logger),
        dir,
        &["checkout", &format!("origin/{branch}"), "--", "staging/"],
        NETWORK_TIMEOUT,
    )?;
    if !checkout_res.ok {
        logger.done(false);
        return Err(format!("The packages of the branch {branch} could not be taken out: {}", checkout_res.error("git checkout")));
    }

    // Collect all .deb files from staging/
    let mut deb_paths = Vec::new();
    if let Ok(entries) = fs::read_dir(&staging_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("deb") {
                deb_paths.push(p.to_string_lossy().to_string());
            }
        }
    }

    if deb_paths.is_empty() {
        let _ = fs::remove_dir_all(&staging_dir);
        let _ = git_logged(Some(&logger), dir, &["reset", "HEAD", "--", "staging/"], API_TIMEOUT);
        logger.done(false);
        return Err("The branch of the proposal has no .deb files".to_string());
    }

    // Optional pre-clean: wipe existing packages so obsolete kernel versions don't linger
    if clean_first {
        logger.step("preClean", &[]);
        publish::wipe_packages_and_reinit(dir)?;
    }

    logger.step("include", &[("count", &deb_paths.len().to_string())]);
    // Import into reprepro
    let dir_text = dir.to_string_lossy().to_string();
    let layout = repos::layout(dir);
    for p in &deb_paths {
        let args = ["-b", &dir_text, "--ignore=unknownfield", "--export=silent-never", "includedeb", &layout.codename, p];
        let r = run("reprepro", &args, None, None, NETWORK_TIMEOUT)?;
        if !r.ok {
            logger.step("repreproWarning", &[("error", &r.error("reprepro"))]);
        }
    }

    // Clean up temporary staging/ folder and unstage
    let _ = fs::remove_dir_all(&staging_dir);
    let _ = git_logged(Some(&logger), dir, &["reset", "HEAD", "--", "staging/"], API_TIMEOUT);

    // Export signed indexes, regenerate appstream and push to main
    logger.step("signIndexes", &[]);
    repos::export_signed(dir)?;

    logger.step("pushMain", &[]);
    publish::publish(Some(&logger), &repo, &format!("Accept proposal #{pr_number}: {}\n\nBranch: {branch}", repo.name))?;

    // Close Pull Request on GitHub
    logger.step("closePr", &[("number", &pr_number.to_string())]);
    let close_body = serde_json::json!({ "state": "closed" });
    let _ = api_patch(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}"), &close_body);

    // Add acceptance comment
    let comment_body = serde_json::json!({
        "body": comment.trim()
    });
    let _ = api_post(&format!("repos/{owner}/{repo_name}/issues/{pr_number}/comments"), &comment_body);

    // Delete remote proposal branch
    logger.step("deleteBranch", &[("branch", &branch)]);
    let _ = git_logged(Some(&logger), dir, &["push", "origin", "--delete", &branch], NETWORK_TIMEOUT);

    logger.step("accepted", &[]);
    logger.done(true);
    Ok(())
}

/// Rejects a proposal: adds a comment with the reason and closes the Pull Request on GitHub.
#[tauri::command(async)]
pub fn reject_proposal(name: String, pr_number: u64, comment: String) -> Result<(), String> {
    let repo = repos::by_name(&name)?;
    let (owner, repo_name) = parse_github_owner_repo(&repo.git_url)
        .ok_or_else(|| "The repository has no valid GitHub URL".to_string())?;

    // Post comment
    // Written by the interface, in its language, with the reason if there is one.
    let comment = comment.trim().to_string();
    let _ = api_post(
        &format!("repos/{owner}/{repo_name}/issues/{pr_number}/comments"),
        &serde_json::json!({ "body": comment }),
    );

    // Close PR
    let close_body = serde_json::json!({ "state": "closed" });
    let _ = api_patch(&format!("repos/{owner}/{repo_name}/pulls/{pr_number}"), &close_body)?;

    Ok(())
}
