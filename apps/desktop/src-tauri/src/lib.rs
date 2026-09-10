mod codex_projection;
mod logging;
mod runtime;
mod secrets;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(runtime::browser::BrowserState::default())
        .manage(runtime::interactive_terminal::PtyState::default())
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.state::<runtime::interactive_terminal::PtyState>().close_all();
                for (label, browser) in window.app_handle().webviews() {
                    if label.starts_with("simple-browser-") {
                        let _ = browser.close();
                    }
                }
            }
        })
        .setup(|app| {
            let base_directory = app.path().app_local_data_dir()?;
            let resource_directory = app.path().resource_dir()?;
            let startup =
                runtime::kernel_desktop::Startup::resolve(&base_directory, &resource_directory)
                    .map_err(std::io::Error::other)?;
            let data_directory = startup.data;
            logging::initialize(data_directory.join("local-agent.jsonl"));
            let state = runtime::DesktopState::open(
                &data_directory.join("local-agent.db"),
                startup.selection,
            )
            .map_err(std::io::Error::other)?;
            if startup.preview {
                runtime::kernel_desktop::import_profiles(
                    &state,
                    &base_directory.join("local-agent.db"),
                )
                .map_err(std::io::Error::other)?;
                logging::info(
                    "kernel_preview_started",
                    serde_json::json!({"notice":runtime::kernel_desktop::PREVIEW_NOTICE}),
                );
            }
            runtime::resume_pending_continuations(app.handle(), &state);
            app.manage(state);
            Ok(())
        })
        .invoke_handler({
            let commands: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
                runtime::load_snapshot,
                runtime::install_workspace_sandbox,
                runtime::load_action,
                runtime::open_project,
                runtime::pick_attachments,
                runtime::image_attachments::pick_images,
                runtime::image_attachments::paste_image,
                runtime::image_attachments::read_image_attachment,
                runtime::browser::browser_command,
                runtime::browser::browser_status,
                runtime::browser::browser_viewport,
                runtime::browser::access::browser_access_pending,
                runtime::browser::access::browser_access_resolve,
                runtime::browser::access::browser_access_policy,
                runtime::read_project_file,
                runtime::load_workspace_diff,
                runtime::interactive_terminal::pty_open,
                runtime::interactive_terminal::pty_read,
                runtime::interactive_terminal::pty_write,
                runtime::interactive_terminal::pty_resize,
                runtime::interactive_terminal::pty_close,
                runtime::open_terminal,
                runtime::run_terminal_command,
                runtime::export_conversation,
                runtime::export_diagnostics,
                runtime::record_frontend_log,
                runtime::branch_conversation,
                runtime::create_task,
                runtime::set_task_permission,
                runtime::save_model_profile,
                runtime::select_model_profile,
                runtime::load_agent_capabilities,
                runtime::set_task_memory_enabled,
                runtime::reset_local_memory,
                runtime::load_project_memory,
                runtime::memory_notes::load_project_memory_notes,
                runtime::memory_notes::save_project_memory_notes,
                runtime::save_local_mcp_server,
                runtime::remove_local_mcp_server,
                runtime::start_chat,
                runtime::start_turn,
                runtime::regenerate_response,
                runtime::revise_message,
                runtime::cancel_turn,
                runtime::approve_action,
                runtime::reject_action,
                runtime::undo_action,
                runtime::cancel_action
            ];
            move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
                // Application commands are only for our trusted main UI. This
                // also denies custom commands that have no plugin ACL entry.
                if invoke.message.webview_ref().label() != "main" {
                    invoke.resolver.reject("网页不能调用 Simple 桌面命令");
                    true
                } else {
                    commands(invoke)
                }
            }
        })
        .run(tauri::generate_context!())
}
