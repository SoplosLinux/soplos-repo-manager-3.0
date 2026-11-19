mod commands;
mod system_theme;

/// Entry point of `--check-proposals`, run by the systemd user timer.
pub fn check_proposals() -> i32 {
    commands::notify::check_and_notify()
}

fn wants_proposals(args: &[String]) -> bool {
    args.iter().skip(1).any(|arg| arg == "--proposals")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The button of the notification starts the program on Proposals.
    commands::notify::remember_start_on_proposals(wants_proposals(&std::env::args().collect::<Vec<_>>()));
    tauri::Builder::default()
        // Must be the first plugin registered: it needs to intercept and
        // exit a second launch before anything else runs.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            use tauri::{Emitter, Manager};
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            // The notification was pressed with the program already open.
            if wants_proposals(&args) {
                let _ = app.emit("show-view", "proposals");
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            system_theme::get_system_theme,
            commands::environment::get_environment_info,
            commands::credentials::has_github_token,
            commands::notify::pending_proposals,
            commands::notify::take_start_on_proposals,
            commands::credentials::has_keyring,
            commands::check::check_packages,
            commands::build::build_package,
            commands::build::host_architecture,
            commands::build::load_maintainer,
            commands::build::save_maintainer,
            commands::build::pick_file,
            commands::build::open_folder,
            commands::credentials::get_github_user,
            commands::credentials::list_accounts,
            commands::credentials::add_account,
            commands::credentials::set_active_account,
            commands::credentials::remove_account,
            commands::credentials::rename_account,
            commands::credentials::get_account_user,
            commands::targets::list_targets,
            commands::targets::save_target,
            commands::targets::remove_target,
            commands::repos::list_repos,
            commands::repos::set_active_repo,
            commands::repos::create_repo,
            commands::repos::update_repo,
            commands::repos::remove_repo,
            commands::repos::pick_folder,
            commands::repos::generate_install_script,
            commands::packages::list_packages,
            commands::packages::scan_packages,
            commands::packages::find_debs,
            commands::packages::add_packages,
            commands::packages::remove_package,
            commands::packages::pick_deb_files,
            commands::packages::republish,
            commands::publish::publish_states,
            commands::publish::get_cleanup_stats,
            commands::publish::clean_repo_normal,
            commands::publish::clean_repo_history,
            commands::proposals::list_proposals,
            commands::proposals::send_proposal,
            commands::proposals::accept_proposal,
            commands::proposals::reject_proposal,
            commands::run::cancel_current_job,
            commands::gpg::list_signing_keys,
            commands::gpg::create_signing_key,
            commands::gpg::import_signing_key,
            commands::gpg::export_public_key,
            commands::gpg::backup_secret_key,
            commands::gpg::delete_signing_key,
            commands::gpg::pick_key_file,
            commands::gpg::pick_save_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Soplos Repo Manager");
}
