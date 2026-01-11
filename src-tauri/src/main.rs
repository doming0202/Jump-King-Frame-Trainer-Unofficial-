#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter};
use serde::Serialize;

use rdev::{listen, EventType, Key};
use gilrs::{Gilrs, EventType as GilEvent, Button as GilButton};
use rodio::{OutputStream, OutputStreamHandle, Sink, source::SineWave, Source};

// ===============================
// HUD Payload
// ===============================
#[derive(Serialize, Clone)]
struct Payload {
    frame: u32,
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

struct AudioState {
    _stream: OutputStream,
    handle: OutputStreamHandle,
}

impl AudioState {
    fn new() -> Self {
        let (_stream, handle) =
            OutputStream::try_default().expect("failed to init audio output");
        Self { _stream, handle }
    }
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
    audio: Arc<AudioState>,
) {
    thread::spawn(move || {
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

                            // HUD progress
                            emit_progress(&app, frame_u);

                            // ===== Zone 音（変化時のみ）=====
                            let zone = get_zone(frame_u);
                            if zone != s.last_zone {
                                match zone {
                                    Zone::Tap   => audio_tx.send(AudioCmd::Beep { freq: 220, ms: 40 }).ok(),
                                    Zone::Small => audio_tx.send(AudioCmd::Beep { freq: 260, ms: 40 }).ok(),
                                    Zone::Mid   => audio_tx.send(AudioCmd::Beep { freq: 300, ms: 40 }).ok(),
                                    Zone::Large => audio_tx.send(AudioCmd::Beep { freq: 340, ms: 40 }).ok(),
                                    Zone::Full  => audio_tx.send(AudioCmd::Beep { freq: 420, ms: 60 }).ok(),
                                }
                                s.last_zone = zone;
                            }

                            // ===== 30F 警告音（1回のみ）=====
                            if frame_u >= 30 && !s.played_30f {
                                audio_tx.send(AudioCmd::Beep { freq: 350, ms: 80 }).ok();
                                s.played_30f = true;
                            }
                        }
                    }
                }
            }

            // ≒240Hz（精度と負荷のバランス）
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
    audio_tx: mpsc::Sender<AudioCmd>,
) {
    thread::spawn(move || {
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
                            audio_tx.send(AudioCmd::Beep { freq: 600, ms: 100 }).ok(); // final音
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
    audio_tx: mpsc::Sender<AudioCmd>,
) {
    thread::spawn(move || {
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
                                audio_tx.send(AudioCmd::Beep { freq: 600, ms: 100 }).ok(); // final音
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
// Main
// ===============================
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let state = Arc::new(Mutex::new(HoldState::default()));
            let (audio_tx, audio_rx) = mpsc::channel();
            start_audio_thread(audio_rx);

            start_frame_loop(handle.clone(), state.clone(), audio_tx.clone());
            start_keyboard_listener(handle.clone(), state.clone(), audio_tx.clone());
            start_gamepad_listener(handle, state, audio_tx);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
