// Telling the administrator that the team sent proposals.
//
// A systemd user timer runs `soplos-repo-manager --check-proposals` every few
// minutes: it asks GitHub for the open pull requests of the repositories of
// this user, with the token of the keyring, and shows a desktop notification
// for the ones it has not told yet. Its button opens the program on the
// Proposals page. The window shows the number of open proposals next to that
// page too.

use super::{proposals, repos};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// The open proposals of every repository of this user that is on GitHub, as
/// "owner/repo#number" with their title.
fn open_proposals() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for repo in repos::load() {
        let Some((owner, name)) = proposals::parse_github_owner_repo(&repo.git_url) else { continue };
        // What failed is written to the log of the timer (journalctl --user),
        // so a check that finds nothing can be told from one that could not ask.
        let items = match proposals::open_pull_requests(&owner, &name) {
            Ok(items) => items,
            Err(error) => {
                eprintln!("{owner}/{name}: {error}");
                continue;
            }
        };
        eprintln!("{owner}/{name}: {} open", items.len());
        for pr in items {
            let number = pr.get("number").and_then(|v| v.as_u64()).unwrap_or(0);
            if number == 0 {
                continue;
            }
            let title = pr.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            found.push((format!("{owner}/{name}#{number}"), title));
        }
    }
    found
}

/// How many proposals are open, for the number next to the Proposals page.
#[tauri::command(async)]
pub fn pending_proposals() -> u32 {
    if super::credentials::active_token().is_none() {
        return 0;
    }
    open_proposals().len() as u32
}

/// The proposals already told, so each one is told once.
fn seen_file() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    base.join("soplos-repo-manager").join("proposals-seen.json")
}

/// The texts of the notification, in the language of the session. The window
/// is translated by the interface; this runs with no window.
struct Texts {
    one: &'static str,
    many: &'static str,
    open: &'static str,
}

fn texts() -> Texts {
    let language = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    match language.get(..2).unwrap_or("") {
        "es" => Texts { one: "Una propuesta nueva del equipo", many: "{n} propuestas nuevas del equipo", open: "Abrir" },
        "de" => Texts { one: "Ein neuer Vorschlag des Teams", many: "{n} neue Vorschläge des Teams", open: "Öffnen" },
        "fr" => Texts { one: "Une nouvelle proposition de l'équipe", many: "{n} nouvelles propositions de l'équipe", open: "Ouvrir" },
        "it" => Texts { one: "Una nuova proposta del team", many: "{n} nuove proposte del team", open: "Apri" },
        "pt" => Texts { one: "Uma nova proposta da equipa", many: "{n} novas propostas da equipa", open: "Abrir" },
        "ro" => Texts { one: "O propunere nouă a echipei", many: "{n} propuneri noi ale echipei", open: "Deschide" },
        "ru" => Texts { one: "Новое предложение команды", many: "Новых предложений команды: {n}", open: "Открыть" },
        _ => Texts { one: "A new proposal from the team", many: "{n} new proposals from the team", open: "Open" },
    }
}

/// Entry point of `--check-proposals`, run by the systemd user timer. It only
/// needs the network and the keyring; without a GitHub account it does nothing.
pub fn check_and_notify() -> i32 {
    if super::credentials::active_token().is_none() {
        eprintln!("No GitHub account in the keyring: nothing to check");
        return 0;
    }
    let open = open_proposals();
    let file = seen_file();
    let seen: BTreeSet<String> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let fresh: Vec<&(String, String)> = open.iter().filter(|(id, _)| !seen.contains(id)).collect();

    // What is open now is what has been told: a proposal that was closed is
    // forgotten, and comes back as new only if it is opened again.
    let now: BTreeSet<String> = open.iter().map(|(id, _)| id.clone()).collect();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = serde_json::to_string(&now) {
        let _ = std::fs::write(&file, body);
    }

    if fresh.is_empty() {
        return 0;
    }
    let texts = texts();
    let summary = if fresh.len() == 1 { texts.one.to_string() } else { texts.many.replace("{n}", &fresh.len().to_string()) };
    let body: Vec<String> = fresh
        .iter()
        .take(5)
        .map(|(id, title)| {
            // "SoplosLinux/soplos-kernels#7" is shown as "soplos-kernels #7".
            let short = id.split_once('/').map(|(_, rest)| rest.replace('#', " #")).unwrap_or_else(|| id.clone());
            format!("{title} ({short})")
        })
        .collect();

    let mut notification = notify_rust::Notification::new();
    notification
        .appname("Soplos Repo Manager")
        .summary(&summary)
        .body(&body.join("\n"))
        .icon("soplos-repo-manager")
        .hint(notify_rust::Hint::DesktopEntry("soplos-repo-manager".into()))
        .action("default", texts.open);
    match notification.show() {
        Ok(handle) => {
            handle.wait_for_action(|action| {
                if action == "default" {
                    let program = std::env::current_exe()
                        .map(|path| path.to_string_lossy().trim_end_matches(" (deleted)").to_string())
                        .unwrap_or_else(|_| "soplos-repo-manager".to_string());
                    let _ = std::process::Command::new(program).arg("--proposals").spawn();
                }
            });
            0
        }
        Err(error) => {
            eprintln!("soplos-repo-manager: the notification could not be shown: {error}");
            1
        }
    }
}

/// Whether the program was asked to open on the Proposals page, by the
/// notification. Answers true only once.
static START_ON_PROPOSALS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn remember_start_on_proposals(value: bool) {
    START_ON_PROPOSALS.store(value, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
pub fn take_start_on_proposals() -> bool {
    START_ON_PROPOSALS.swap(false, std::sync::atomic::Ordering::Relaxed)
}
