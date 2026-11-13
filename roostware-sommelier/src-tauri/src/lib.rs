// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

pub mod commands;
pub mod db;
pub mod leetcode;

use tauri::Manager;

/// Registers every command. Shared by `run` and the IPC tests so they can't drift apart.
pub fn with_commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        greet,
        commands::list_problems,
        commands::get_problem,
        commands::get_problem_by_slug,
        commands::list_tags,
        commands::list_collections,
        commands::record_problem_opened,
        commands::list_recent_problems,
        commands::save_draft,
        commands::delete_draft,
        commands::leetcode_status,
        commands::leetcode_sign_in,
        commands::leetcode_sign_out,
        commands::run_code,
        commands::submit_code,
        commands::list_submissions,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    with_commands(tauri::Builder::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(db::Db::open(app.handle())?);
            app.manage(leetcode::LeetCode::production(app.handle()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
