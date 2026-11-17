// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

pub mod coach;
pub mod commands;
pub mod db;
pub mod leetcode;
pub mod recommend;

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
        commands::create_collection,
        commands::add_problem_to_collection,
        commands::remove_problem_from_collection,
        commands::record_problem_opened,
        commands::list_recent_problems,
        commands::save_draft,
        commands::delete_draft,
        commands::leetcode_status,
        commands::leetcode_sign_in,
        commands::leetcode_sign_out,
        commands::reset_user_data,
        commands::run_code,
        commands::submit_code,
        commands::list_submissions,
        commands::list_submission_times,
        commands::get_daily_recommendations,
        commands::get_pattern_strengths,
        commands::get_rating,
        commands::set_review_grade,
        commands::get_settings,
        commands::set_daily_problem_count,
        commands::set_show_curated_collections,
        commands::set_coach_settings,
        commands::give_up,
        commands::get_give_up_info,
        coach::commands::coach_status,
        coach::commands::set_coach_key,
        coach::commands::clear_coach_key,
        coach::commands::test_coach_connection,
        coach::commands::coach_reply,
        coach::commands::coach_cancel,
        coach::commands::load_coach_thread,
        coach::commands::save_coach_message,
        coach::commands::save_coach_state,
        coach::commands::new_coach_thread,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    with_commands(tauri::Builder::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(db::Db::open(app.handle())?);
            app.manage(leetcode::LeetCode::production(app.handle()));
            app.manage(coach::Coach::production(app.handle()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
