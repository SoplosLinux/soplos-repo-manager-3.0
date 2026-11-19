// Entry point delegates to the library so integration tests can reuse it.

fn main() {
    // WebKitGTK's DMA-BUF renderer causes protocol errors (Error 71) on Wayland
    // with NVIDIA/Nouveau drivers on physical hardware, as well as blank windows
    // on virtual GPUs. Force it disabled by default on all hardware.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // The systemd user timer asks for the proposals of the team, with no
    // window: it notifies the new ones and exits.
    if std::env::args().nth(1).as_deref() == Some("--check-proposals") {
        std::process::exit(soplos_repo_manager_lib::check_proposals());
    }

    soplos_repo_manager_lib::run();
}
