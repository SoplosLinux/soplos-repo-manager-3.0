// Publishing a repository on its Git platform. The Git copy holds a single
// commit with the files of now, pushed over the one on the platform: apt only
// reads the current files, and the old packages are kept elsewhere, so the
// history of Git only made the repositories grow (6 GB for 300 MB of packages).

use super::credentials;
use super::repos::{self, Repo};
use super::run::{run, run_logged_env, JobLogger, Run};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

const QUICK: Duration = Duration::from_secs(60);
// Pushing a whole repository of kernels can take long.
const PUSH: Duration = Duration::from_secs(3600);

/// Runs git in `dir`, never asking for a password on a terminal that nobody sees.
fn git(dir: &Path, args: &[&str], timeout: Duration) -> Result<Run, String> {
    let dir_text = dir.to_string_lossy().to_string();
    let mut full = vec!["GIT_TERMINAL_PROMPT=0", "git", "-C", &dir_text];
    full.extend_from_slice(args);
    run("env", &full, None, None, timeout)
}

fn _git_ok(dir: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let r = git(dir, args, timeout)?;
    if r.ok {
        Ok(r.stdout)
    } else {
        Err(r.error("git"))
    }
}

pub(crate) fn git_logged(logger: Option<&JobLogger>, dir: &Path, args: &[&str], timeout: Duration) -> Result<Run, String> {
    // The token of the account in use goes in the environment, as an HTTP
    // header for github.com: never on the command line, the log or .git/config.
    let mut envs = credentials::git_auth_env();
    envs.push(("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()));
    run_logged_env(logger, "git", args, Some(dir), None, &envs, timeout)
}

pub(crate) fn git_logged_ok(logger: Option<&JobLogger>, dir: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let r = git_logged(logger, dir, args, timeout)?;
    if r.ok {
        Ok(r.stdout)
    } else {
        Err(r.error("git"))
    }
}

fn branch_of(repo: &Repo) -> String {
    if repo.branch.trim().is_empty() {
        "main".to_string()
    } else {
        repo.branch.trim().to_string()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishState {
    pub name: String,
    /// It has a Git address to publish to.
    pub configured: bool,
    /// Its Git copy has more than one commit: the first publication replaces them.
    pub has_history: bool,
}

pub fn state(repo: &Repo) -> PublishState {
    let dir = Path::new(&repo.dir);
    let has_history = dir.join(".git").exists()
        && git(dir, &["rev-list", "--count", "HEAD"], QUICK)
            .ok()
            .and_then(|r| r.stdout.trim().parse::<u64>().ok())
            .is_some_and(|n| n > 1);
    PublishState { name: repo.name.clone(), configured: !repo.git_url.trim().is_empty(), has_history }
}

#[tauri::command(async)]
pub fn publish_states(names: Vec<String>) -> Vec<PublishState> {
    names.iter().filter_map(|n| repos::by_name(n).ok()).map(|r| state(&r)).collect()
}

/// Replaces what the platform has with the files of now, in one commit.
/// Publishes pending changes incrementally to the Git platform.
pub fn publish(logger: Option<&JobLogger>, repo: &Repo, message: &str) -> Result<(), String> {
    let dir = Path::new(&repo.dir);
    let branch = branch_of(repo);
    if !dir.join(".git").exists() {
        git_logged_ok(logger, dir, &["init", "-q", "-b", &branch], QUICK)?;
    }
    let remotes = git_logged_ok(logger, dir, &["remote"], QUICK)?;
    if !remotes.lines().any(|r| r.trim() == "origin") {
        let url = credentials::clean_url(&repo.git_url);
        git_logged_ok(logger, dir, &["remote", "add", "origin", &url], QUICK)?;
    } else {
        // The URL without any token an earlier version wrote into .git/config.
        let url = credentials::clean_url(&repo.git_url);
        let _ = git_logged(logger, dir, &["remote", "set-url", "origin", &url], QUICK);
    }

    // Ensure we are on the target branch
    let current = git_logged_ok(logger, dir, &["branch", "--show-current"], QUICK).unwrap_or_default();
    if current.trim() != branch {
        let _ = git_logged(logger, dir, &["checkout", &branch], QUICK);
    }

    // Add all current changes (dists, pool, appstream, media, etc.)
    if let Some(l) = logger {
        l.step("gitAdd", &[]);
    }
    git_logged_ok(logger, dir, &["add", "-A"], PUSH)?;

    // Only commit if there are staged changes
    let status = git_logged_ok(logger, dir, &["status", "--porcelain"], QUICK)?;
    if !status.trim().is_empty() {
        if let Some(l) = logger {
            l.step("commit", &[("message", message)]);
        }
        git_logged_ok(logger, dir, &["commit", "-m", message], PUSH)?;
    }

    // Standard incremental push
    if let Some(l) = logger {
        l.step("push", &[("branch", &branch)]);
    }
    let push_res = git_logged(logger, dir, &["push", "-u", "origin", &branch], PUSH)?;
    if !push_res.ok {
        // Fallback: try plain push without -u
        let push_fallback = git_logged(logger, dir, &["push", "origin", &branch], PUSH)?;
        if !push_fallback.ok {
            return Err(push_res.error("git push"));
        }
    }
    if let Some(l) = logger {
        l.step("gitDone", &[]);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanRepoStats {
    pub repo: String,
    pub pool_size: u64,
    pub git_size: u64,
    pub total_size: u64,
    pub commit_count: u64,
}

fn walkdir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                total += path.metadata().map(|m| m.len()).unwrap_or(0);
            } else if path.is_dir() {
                total += walkdir_size(&path);
            }
        }
    }
    total
}

#[tauri::command(async)]
pub fn get_cleanup_stats(name: String) -> Result<CleanRepoStats, String> {
    let repo = repos::by_name(&name)?;
    let dir = Path::new(&repo.dir);
    let git_dir = dir.join(".git");
    let pool_dir = dir.join("pool");

    let git_size = if git_dir.exists() { walkdir_size(&git_dir) } else { 0 };
    let pool_size = if pool_dir.exists() { walkdir_size(&pool_dir) } else { 0 };
    let total_size = if dir.exists() { walkdir_size(dir) } else { 0 };

    let commit_count = if git_dir.exists() {
        git(dir, &["rev-list", "--count", "HEAD"], QUICK)
            .ok()
            .and_then(|r| r.stdout.trim().parse::<u64>().ok())
            .unwrap_or(0)
    } else {
        0
    };

    Ok(CleanRepoStats {
        repo: name,
        pool_size,
        git_size,
        total_size,
        commit_count,
    })
}

pub(crate) fn wipe_packages_and_reinit(dir: &Path) -> Result<(), String> {
    let dirs = ["pool", "db", "dists", "media", "appstream", "tmp", "incoming"];
    for d in &dirs {
        let p = dir.join(d);
        if p.exists() {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
    std::fs::create_dir_all(dir.join("pool/main")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("media")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("appstream")).map_err(|e| e.to_string())?;

    // Export empty signed indexes so the repository structure is valid
    let _ = repos::export_signed(dir);
    Ok(())
}

/// Performs a standard cleanup: wipes all packages and metadata while preserving
/// configuration (conf/distributions, GPG keys) and pushes a clean commit without
/// altering Git history.
#[tauri::command(async)]
pub fn clean_repo_normal(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let logger = JobLogger::new_step(app, "cleanTitle", &[("repo", &name)]);
    logger.step("cleanStart", &[("repo", &name)]);
    let repo = repos::by_name(&name)?;
    let dir = Path::new(&repo.dir);
    let branch = branch_of(&repo);
    wipe_packages_and_reinit(dir)?;
    logger.step("cleanEmptied", &[]);

    if dir.join(".git").exists() {
        git_logged_ok(Some(&logger), dir, &["add", "-A"], PUSH)?;
        let status = git_logged_ok(Some(&logger), dir, &["status", "--porcelain"], QUICK)?;
        if !status.trim().is_empty() {
            logger.step("cleanCommit", &[]);
            git_logged_ok(Some(&logger), dir, &["commit", "-m", "Empty the repository"], PUSH)?;
            logger.step("cleanPush", &[]);
            let push_res = git_logged(Some(&logger), dir, &["push", "origin", &branch], PUSH)?;
            if !push_res.ok {
                logger.done(false);
                return Err(push_res.error("git push"));
            }
        }
    }
    logger.step("cleanDone", &[]);
    logger.done(true);
    Ok(())
}

/// Performs a deep cleanup: wipes all packages and metadata, then squashes the
/// Git history into a single clean initial orphan commit and force-pushes it.
/// Because packages are wiped first, the push contains only configuration (~50 KB)
/// and finishes in seconds, leaving both local disk and GitHub minimal.
#[tauri::command(async)]
pub fn clean_repo_history(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let logger = JobLogger::new_step(app, "deepTitle", &[("repo", &name)]);
    logger.step("deepStart", &[("repo", &name)]);
    let repo = repos::by_name(&name)?;
    let dir = Path::new(&repo.dir);
    let branch = branch_of(&repo);
    wipe_packages_and_reinit(dir)?;
    logger.step("deepEmptied", &[]);

    if !dir.join(".git").exists() {
        logger.done(true);
        return Ok(());
    }

    let work = "soplos-repo-manager-clean";
    let _ = git(dir, &["branch", "-D", work], QUICK);
    git_logged_ok(Some(&logger), dir, &["checkout", "-q", "--orphan", work], QUICK)?;
    let made = (|| -> Result<(), String> {
        git_logged_ok(Some(&logger), dir, &["add", "-A"], PUSH)?;
        git_logged_ok(Some(&logger), dir, &["commit", "-m", "Historial limpio completo - repositorio vaciado"], PUSH)?;
        let _ = git(dir, &["branch", "-D", &branch], QUICK);
        git_logged_ok(Some(&logger), dir, &["branch", "-m", &branch], QUICK)?;
        Ok(())
    })();
    if let Err(e) = made {
        let _ = git(dir, &["checkout", "-q", &branch], QUICK);
        logger.done(false);
        return Err(e);
    }
    logger.step("deepForce", &[]);
    let push_res = git_logged(Some(&logger), dir, &["push", "--force", "origin", &branch], PUSH)?;
    if !push_res.ok {
        logger.done(false);
        return Err(push_res.error("git push"));
    }
    logger.step("deepGc", &[]);
    let _ = git_logged(Some(&logger), dir, &["reflog", "expire", "--expire=now", "--all"], PUSH);
    let _ = git_logged(Some(&logger), dir, &["gc", "--prune=now"], PUSH);
    logger.step("deepDone", &[]);
    logger.done(true);
    Ok(())
}
