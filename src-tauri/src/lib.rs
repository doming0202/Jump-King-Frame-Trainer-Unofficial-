use tauri::{Manager};

#[cfg_attr(not(debug_assertions), tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // setup 処理があればここ
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
