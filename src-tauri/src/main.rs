#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, State, Manager};
use serde::Serialize;

use rdev::{listen, EventType, Key};
use gilrs::{Gilrs, EventType as GilEvent, Button as GilButton};
use rodio::{OutputStream, Sink, source::SineWave, Source};

// ===============================
// HUD Payload
// ===============================
#[derive(Serialize, Clone)]
struct Payload {
    frame: u32,
}

// ===============================
// HUD Control State
// ===============================
struct HudControlState {
    visible: bool,
    muted: bool,
}

// ===============================
// Jump Zones（JS側と同一）
// ===============================
#[derive(Clone, Copy, PartialEq)]
enum Zone {
    None,
    Tap,
    Small,
    Mid,
    Large,
    Full,
}

fn get_zone(frame: u32) -> Zone {
    match frame {
        36.. => Zone::Full,
        25..=35 => Zone::Large,
        14..=24 => Zone::Mid,
        8..=13 => Zone::Small,
        1..=7 => Zone::Tap,
        _ => Zone::None,
    }
}

// ===============================
// Shared State
// ===============================
struct HoldState {
    holding: bool,
    start: Option<Instant>,
    last_frame: i32,

    last_zone: Zone,
    played_30f: bool,
}

impl Default for HoldState {
    fn default() -> Self {
        Self {
            holding: false,
            start: None,
            last_frame: -1,
            last_zone: Zone::None,
            played_30f: false,
        }
    }
}

enum AudioCmd {
    Beep { freq: u32, ms: u64 },
}

// ===============================
// Sound Helper（JSと同思想）
// ===============================
fn start_audio_thread(rx: mpsc::Receiver<AudioCmd>) {
    thread::spawn(move || {
        let (_stream, handle) =
            OutputStream::try_default().expect("failed to init audio output");
        while let Ok(cmd) = rx.recv() {
            match cmd {
                AudioCmd::Beep { freq, ms } => {
                    if let Ok(sink) = Sink::try_new(&handle) {
                        sink.append(
                            SineWave::new(freq as f32)
                                .take_duration(Duration::from_millis(ms))
                                .amplify(0.20)
                        );
                        sink.detach();
                    }
                }
            }
        }
    });
}

// ===============================
// HUD Emit
// ===============================
fn emit_progress(app: &AppHandle, frame: u32) {
    let _ = app.emit_to("hud", "hud-progress", Payload { frame });
}

fn emit_update(app: &AppHandle, frame: u32) {
    let _ = app.emit_to("hud", "hud-update", Payload { frame });
}

// ===============================
// Frame Loop（60FPS基準）
// ===============================
fn start_frame_loop(
    app: AppHandle,
    state: Arc<Mutex<HoldState>>,
    hud_state: Arc<Mutex<HudControlState>>,
    audio_tx: mpsc::Sender<AudioCmd>,
) {
    thread::spawn(move || {
        let audio_tx = audio_tx.clone(); // ★これが重要
        const FRAME_MS: f64 = 1000.0 / 60.0;

        loop {
            {
                let mut s = state.lock().unwrap();
                if s.holding {
                    if let Some(start) = s.start {
                        let elapsed_ms =
                            start.elapsed().as_secs_f64() * 1000.0;
                        let frame =
                            (elapsed_ms / FRAME_MS).floor() as i32;

                        if frame != s.last_frame && frame >= 0 {
                            s.last_frame = frame;
                            let frame_u = frame as u32;

                            emit_progress(&app, frame_u);

                            let zone = get_zone(frame_u);
                            if zone != s.last_zone {
                                if hud_state.lock().unwrap().muted {
                                    s.last_zone = zone;
                                    continue;
                                }
                                match zone {
                                    Zone::Tap => {
                                        audio_tx.send(AudioCmd::Beep { freq: 220, ms: 40 }).ok();
                                    }
                                    Zone::Small => {
                                        audio_tx.send(AudioCmd::Beep { freq: 260, ms: 40 }).ok();
                                    }
                                    Zone::Mid => {
                                        audio_tx.send(AudioCmd::Beep { freq: 300, ms: 40 }).ok();
                                    }
                                    Zone::Large => {
                                        audio_tx.send(AudioCmd::Beep { freq: 340, ms: 40 }).ok();
                                    }
                                    Zone::Full => {
                                        audio_tx.send(AudioCmd::Beep { freq: 420, ms: 60 }).ok();
                                    }
                                    Zone::None => {}
                                }

                                s.last_zone = zone;
                            }

                            if frame_u >= 30 && !s.played_30f {
                                if hud_state.lock().unwrap().muted {
                                    s.played_30f = true;
                                    continue;
                                }
                                audio_tx.send(AudioCmd::Beep { freq: 350, ms: 80 }).ok();
                                s.played_30f = true;
                            }
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(4));
        }
    });
}

// ===============================
// Keyboard Listener（Spaceのみ）
// ===============================
fn start_keyboard_listener(
    app: AppHandle,
    state: Arc<Mutex<HoldState>>,
    hud_state: Arc<Mutex<HudControlState>>,
    audio_tx: mpsc::Sender<AudioCmd>,
) {
    thread::spawn(move || {
        let audio_tx = audio_tx.clone();
        let callback = move |event: rdev::Event| {
            match event.event_type {
                EventType::KeyPress(Key::Space) => {
                    let mut s = state.lock().unwrap();
                    if !s.holding {
                        s.holding = true;
                        s.start = Some(Instant::now());
                        s.last_frame = -1;
                        s.last_zone = Zone::None;
                        s.played_30f = false;
                    }
                }

                EventType::KeyRelease(Key::Space) => {
                    let mut s = state.lock().unwrap();
                    if s.holding {
                        if let Some(start) = s.start {
                            let elapsed_ms =
                                start.elapsed().as_secs_f64() * 1000.0;
                            let frame =
                                (elapsed_ms / (1000.0 / 60.0)).round() as u32;

                            emit_update(&app, frame);
                            if !hud_state.lock().unwrap().muted {
                                audio_tx.send(AudioCmd::Beep { freq: 600, ms: 100 }).ok(); // final音
                            }
                        }

                        s.holding = false;
                        s.start = None;
                        s.last_frame = -1;
                        s.last_zone = Zone::None;
                    }
                }

                _ => {}
            }
        };

        let _ = listen(callback);
    });
}

// ===============================
// Gamepad Listener（全機種共通ジャンプ）
// ===============================
fn start_gamepad_listener(
    app: AppHandle,
    state: Arc<Mutex<HoldState>>,
    hud_state: Arc<Mutex<HudControlState>>,
    audio_tx: mpsc::Sender<AudioCmd>,
) {
    thread::spawn(move || {
        let audio_tx = audio_tx.clone();
        let mut gilrs = Gilrs::new().unwrap();

        loop {
            while let Some(ev) = gilrs.next_event() {
                match ev.event {
                    GilEvent::ButtonPressed(GilButton::South, _) => {
                        let mut s = state.lock().unwrap();
                        if !s.holding {
                            s.holding = true;
                            s.start = Some(Instant::now());
                            s.last_frame = -1;
                            s.last_zone = Zone::None;
                            s.played_30f = false;
                        }
                    }

                    GilEvent::ButtonReleased(GilButton::South, _) => {
                        let mut s = state.lock().unwrap();
                        if s.holding {
                            if let Some(start) = s.start {
                                let elapsed_ms =
                                    start.elapsed().as_secs_f64() * 1000.0;
                                let frame =
                                    (elapsed_ms / (1000.0 / 60.0)).round() as u32;

                                emit_update(&app, frame);
                                if !hud_state.lock().unwrap().muted {
                                    audio_tx.send(AudioCmd::Beep { freq: 600, ms: 100 }).ok(); // final音
                                }
                            }

                            s.holding = false;
                            s.start = None;
                            s.last_frame = -1;
                            s.last_zone = Zone::None;
                        }
                    }

                    _ => {}
                }
            }

            thread::sleep(Duration::from_millis(4));
        }
    });
}

// ===============================
// HUD Commands
// ===============================
#[tauri::command]
fn hud_toggle(app: tauri::AppHandle, state: State<Arc<Mutex<HudControlState>>>) {
    let mut s = state.lock().unwrap();
    s.visible = !s.visible;

    if let Some(hud) = app.get_webview_window("hud") {
        if s.visible {
            let _ = hud.show();
        } else {
            let _ = hud.hide();
        }
    }
}

#[tauri::command]
fn hud_mute_toggle(
    app: tauri::AppHandle,
    state: State<Arc<Mutex<HudControlState>>>,
) {
    let mut s = state.lock().unwrap();
    s.muted = !s.muted;

    let _ = app.emit_to("main", "hud-mute-changed", s.muted);
}

// ===============================
// Main
// ===============================
fn main() {
    let hud_state = Arc::new(Mutex::new(HudControlState {
        visible: true,
        muted: false,
    }));

    tauri::Builder::default()
        .manage(hud_state.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = Arc::new(Mutex::new(HoldState::default()));
            let (audio_tx, audio_rx) = mpsc::channel();
            start_audio_thread(audio_rx);

            start_frame_loop(handle.clone(), state.clone(), hud_state.clone(), audio_tx.clone());
            start_keyboard_listener(handle.clone(), state.clone(), hud_state.clone(), audio_tx.clone());
            start_gamepad_listener(handle, state, hud_state, audio_tx);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![hud_toggle, hud_mute_toggle])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
