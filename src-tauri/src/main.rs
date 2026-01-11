#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::thread;
use tauri::{AppHandle, Manager, Emitter};
use rdev::{listen, EventType, Button};

#[tauri::command]
fn hud_progress(app: AppHandle, frame: u32) {
    if let Some(w) = app.get_webview_window("hud") {
        let _ = w.emit("hud-progress", serde_json::json!({
            "frame": frame
        }));
    }
}

#[tauri::command]
fn hud_update(app: AppHandle, frame: u32) {
    if let Some(w) = app.get_webview_window("hud") {
        let _ = w.emit("hud-update", serde_json::json!({
            "frame": frame
        }));
    }
}

fn start_global_input_listener(app: AppHandle) {
    thread::spawn(move || {
        let mut holding = false;

        let callback = move |event: rdev::Event| {
            match event.event_type {
                EventType::KeyPress(_) => {
                    if !holding {
                        holding = true;
                        let _ = app.emit("hold-start", "key");
                    }
                }
                EventType::KeyRelease(_) => {
                    if holding {
                        holding = false;
                        let _ = app.emit("hold-end", ());
                    }
                }
                EventType::ButtonPress(button) => {
                    if button == Button::Left && !holding {
                        holding = true;
                        let _ = app.emit("hold-start", "mouse:left");
                    }
                }
                EventType::ButtonRelease(button) => {
                    if button == Button::Left && holding {
                        holding = false;
                        let _ = app.emit("hold-end", ());
                    }
                }
                _ => {}
            }
        };

        if let Err(err) = listen(callback) {
            eprintln!("Global input error: {:?}", err);
        }
    });
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();

            // ① 既存のグローバル入力リスナー（変更なし）
            start_global_input_listener(handle.clone());

            // ② HUD ウィンドウの存在確認（Discord方式）
            // tauri.conf.json 側で定義されていれば、ここで取得できる
            if let Some(_hud) = app.get_webview_window("hud") {
                // 今は何もしない（表示専用HUD）
                // 将来ここで初期イベントを送れる
                // let _ = _hud.emit("hud-init", ());
            } else {
                eprintln!("HUD window not found");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            hud_progress,
            hud_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
