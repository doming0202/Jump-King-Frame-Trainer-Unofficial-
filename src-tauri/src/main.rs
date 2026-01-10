#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::thread;
use tauri::{AppHandle, Manager, Emitter};
use rdev::{listen, EventType, Button};

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
            start_global_input_listener(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
