// Release builds are desktop apps; keep the console available for development.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> tauri::Result<()> {
    local_agent_desktop::run()
}
