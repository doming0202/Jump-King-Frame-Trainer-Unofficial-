#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::thread;
use tauri::{AppHandle, Manager};
use tauri::Emitter;
use rdev::{listen, EventType, Button};
use serde::Serialize;

// ===============================
// HUD payload
// ===============================
#[derive(Serialize, Clone)]
struct Payload {
    frame: u32,
}

// ===============================
// HUD update command（確定値のみ）
// ===============================
#[tauri::command]
fn hud_update(app: AppHandle, frame: u32) {
    let _ = app.emit_to("hud", "hud-update", Payload { frame });
}

// ===============================
// Global input listener（既存）
// ===============================
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

// ===============================
// main
// ===============================
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();

            // グローバル入力開始（変更なし）
            start_global_input_listener(handle);

            Ok(())
        })
        // ★ invoke 経路を有効化（ここが最重要）
        .invoke_handler(tauri::generate_handler![
            hud_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
