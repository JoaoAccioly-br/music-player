pub mod audio;
pub mod auth;
pub mod innertube;

use audio::{
    extract_youtube_audio_stream_cancellable, parse_youtube_video_id, AudioCommand, AudioEngine,
};
use auth::{
    build_session_from_cookie_str, clear_session_on_disk, load_session_from_disk,
    open_google_login_window, save_session_to_disk, AuthSession, AuthStatus,
};
use innertube::{
    fetch_home, fetch_liked_music, fetch_playlist_tracks, fetch_radio_up_next,
    fetch_user_playlists, search_music, HomeSection, PlaylistItem, TrackItem,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, Wry,
};

/// Static list of real YouTube VOD tracks with real YouTube video IDs and durations:
/// `(video_id, title, artist, duration_secs)`
const DEMO_TRACKS: &[(&str, &str, &str, f64)] = &[
    ("MV_3Dpw-BRY", "Nightcall", "Kavinsky", 258.0),
    ("fJ9rUzIMcZQ", "Bohemian Rhapsody", "Queen", 355.0),
    ("4NRXx6U8ABQ", "Blinding Lights", "The Weeknd", 200.0),
    ("FGBhQbmPwH8", "One More Time", "Daft Punk", 320.0),
];

fn default_queue_tracks() -> Vec<TrackItem> {
    DEMO_TRACKS
        .iter()
        .map(|(vid, title, artist, dur)| {
            let mins = (*dur as u64) / 60;
            let secs = (*dur as u64) % 60;
            TrackItem {
                video_id: (*vid).to_string(),
                title: (*title).to_string(),
                artist: (*artist).to_string(),
                album: "YouTube Music Stream".to_string(),
                duration_text: format!("{mins}:{secs:02}"),
                duration_secs: *dur,
                thumbnail_url: format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg"),
            }
        })
        .collect()
}

/// Phase 5: Repeat/Loop mode persisted in Rust state.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }
}

/// Persistent player state that lives in Rust even when the WebView window is destroyed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub is_playing: bool,
    pub current_track_title: String,
    pub current_track_artist: String,
    pub current_thumbnail_url: String,
    pub volume: f32,
    pub queue_index: usize,
    pub queue_len: usize,
    pub video_id: String,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub is_loading: bool,
    pub audio_quality_label: String,
    pub buffer_size_mb: f32,
    pub repeat_mode: RepeatMode,
    pub autoplay_radio: bool,
    pub low_ram_no_images: bool,
    pub error_message: Option<String>,
}

impl Default for PlayerState {
    fn default() -> Self {
        let (video_id, title, artist, duration_secs) = DEMO_TRACKS[0];
        Self {
            is_playing: false,
            current_track_title: title.to_string(),
            current_track_artist: artist.to_string(),
            current_thumbnail_url: format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"),
            volume: 0.8,
            queue_index: 0,
            queue_len: DEMO_TRACKS.len(),
            video_id: video_id.to_string(),
            position_secs: 0.0,
            duration_secs,
            is_loading: false,
            audio_quality_label: "Pronto (Clique em Play para carregar na RAM)".to_string(),
            buffer_size_mb: 0.0,
            repeat_mode: RepeatMode::Off,
            autoplay_radio: true,
            low_ram_no_images: false,
            error_message: None,
        }
    }
}

impl PlayerState {
    pub fn format_tray_status(&self) -> String {
        if self.is_loading {
            return format!("⏳ Carregando: {}...", self.current_track_title);
        }
        let prefix = if self.is_playing {
            "▶ Tocando"
        } else {
            "⏸ Pausado"
        };
        let radio_badge = if self.autoplay_radio { " [Rádio]" } else { "" };
        format!(
            "{}: {} — {}{}",
            prefix, self.current_track_title, self.current_track_artist, radio_badge
        )
    }

    pub fn apply_track_from_queue(&mut self, queue: &[TrackItem], index: usize) {
        if queue.is_empty() {
            let safe_index = index % DEMO_TRACKS.len();
            let (video_id, title, artist, duration_secs) = DEMO_TRACKS[safe_index];
            self.queue_index = safe_index;
            self.queue_len = DEMO_TRACKS.len();
            self.video_id = video_id.to_string();
            self.current_track_title = title.to_string();
            self.current_track_artist = artist.to_string();
            self.current_thumbnail_url = format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg");
            self.position_secs = 0.0;
            self.duration_secs = duration_secs;
            self.error_message = None;
            return;
        }

        let safe_index = index % queue.len();
        let item = &queue[safe_index];
        self.queue_index = safe_index;
        self.queue_len = queue.len();
        self.video_id = item.video_id.clone();
        self.current_track_title = item.title.clone();
        self.current_track_artist = item.artist.clone();
        self.current_thumbnail_url = item.thumbnail_url.clone();
        self.position_secs = 0.0;
        self.duration_secs = item.duration_secs.max(30.0);
        self.error_message = None;
    }
}

/// Shared application container holding the player state, active queue, Google auth session,
/// dedicated native audio engine thread, tray status menu item handle, deduplicated tray text,
/// download generation counter (for instant cancellation of stale `yt-dlp` processes), and exit flag.
pub struct AppContainer {
    pub player: Mutex<PlayerState>,
    pub queue: Mutex<Vec<TrackItem>>,
    pub auth: Mutex<AuthSession>,
    pub audio_engine: AudioEngine,
    pub tray_status_item: Mutex<Option<MenuItem<Wry>>>,
    pub last_tray_text: Mutex<String>,
    pub download_generation: Arc<AtomicU64>,
    pub quit_requested: AtomicBool,
}

impl Default for AppContainer {
    fn default() -> Self {
        let initial_auth = load_session_from_disk();
        Self {
            player: Mutex::new(PlayerState::default()),
            queue: Mutex::new(default_queue_tracks()),
            auth: Mutex::new(initial_auth),
            audio_engine: AudioEngine::new(),
            tray_status_item: Mutex::new(None),
            last_tray_text: Mutex::new(String::new()),
            download_generation: Arc::new(AtomicU64::new(0)),
            quit_requested: AtomicBool::new(false),
        }
    }
}

/// Synchronizes state changes with both the native System Tray menu and the WebView (if open).
/// Deduplicates `status_item.set_text(...)` via `last_tray_text` so Linux DBus / AppIndicator /
/// GNOME Shell / Xwayland are never spammed when the tray text hasn't changed!
fn broadcast_state_change(app: &AppHandle, state: &PlayerState) {
    if let Some(container) = app.try_state::<AppContainer>() {
        let new_tray_text = state.format_tray_status();
        let should_update_tray = if let Ok(mut last_text) = container.last_tray_text.lock() {
            if *last_text != new_tray_text {
                *last_text = new_tray_text.clone();
                true
            } else {
                false
            }
        } else {
            true
        };

        if should_update_tray {
            if let Ok(guard) = container.tray_status_item.lock() {
                if let Some(status_item) = guard.as_ref() {
                    let _ = status_item.set_text(new_tray_text);
                }
            }
        }
    }

    // Only emit IPC events if the main WebView window is currently open!
    if app.get_webview_window("main").is_some() {
        let _ = app.emit("player-state-changed", state);
    }
}

fn broadcast_queue_change(app: &AppHandle, queue: &[TrackItem]) {
    if app.get_webview_window("main").is_some() {
        let _ = app.emit("queue-changed", queue);
    }
}

fn current_auth_snapshot(app: &AppHandle) -> Option<AuthSession> {
    let container = app.try_state::<AppContainer>()?;
    let guard = container.auth.lock().ok()?;
    if guard.is_authenticated && !guard.sapisid.is_empty() {
        Some(guard.clone())
    } else {
        None
    }
}

/// Spawns an asynchronous background task on Tauri's Tokio runtime to extract the real YouTube audio
/// stream (via isolated `~/.local/share/com.ytmusic.rust/bin/yt-dlp` or Innertube) into a RAM `Vec<u8>`
/// and feed it to `AudioEngine`. Increments `download_generation` so any previous in-flight `yt-dlp`
/// process is immediately terminated (`kill_on_drop`).
fn spawn_track_loader(
    app: AppHandle,
    video_id: String,
    _seed_index: usize,
    fallback_duration: f64,
    allow_metadata_override: bool,
) {
    let auth_opt = current_auth_snapshot(&app);
    let generation_pair = app.try_state::<AppContainer>().map(|c| {
        let gen = c.download_generation.fetch_add(1, Ordering::SeqCst) + 1;
        (gen, c.download_generation.clone())
    });

    tauri::async_runtime::spawn(async move {
        let extraction_result = extract_youtube_audio_stream_cancellable(
            &video_id,
            auth_opt.as_ref(),
            generation_pair.clone(),
        )
        .await;

        let Some(container) = app.try_state::<AppContainer>() else {
            return;
        };

        let updated = {
            let mut state = container.player.lock().expect("player mutex poisoned");
            // Ensure the user hasn't switched to a different video or newer generation while downloading
            if state.video_id != video_id {
                return;
            }
            if let Some((expected_gen, ref counter)) = generation_pair {
                if counter.load(Ordering::SeqCst) != expected_gen {
                    return;
                }
            }

            match extraction_result {
                Ok(extracted) => {
                    let buffer_mb = (extracted.data.len() as f32) / (1024.0 * 1024.0);
                    let current_vol = state.volume;

                    container.audio_engine.send(AudioCommand::LoadAndPlay {
                        data: extracted.data,
                        volume: current_vol,
                    });

                    if allow_metadata_override {
                        if let Some(title) = extracted.title {
                            if !title.trim().is_empty() {
                                state.current_track_title = title;
                            }
                        }
                        if let Some(author) = extracted.author {
                            if !author.trim().is_empty() {
                                state.current_track_artist = author;
                            }
                        }
                    }

                    if extracted.duration_secs > 1.0 {
                        state.duration_secs = extracted.duration_secs;
                    } else if fallback_duration > 1.0 {
                        state.duration_secs = fallback_duration;
                    }
                    state.position_secs = 0.0;
                    state.buffer_size_mb = buffer_mb;
                    state.audio_quality_label = extracted.quality_label;
                    state.is_loading = false;
                    state.is_playing = true;
                    state.error_message = None;
                }
                Err(err_msg) => {
                    container.audio_engine.send(AudioCommand::Stop);
                    state.is_loading = false;
                    state.is_playing = false;
                    state.buffer_size_mb = 0.0;
                    state.audio_quality_label = "Erro ao extrair stream do YouTube".to_string();
                    state.error_message = Some(err_msg);
                }
            }
            state.clone()
        };

        broadcast_state_change(&app, &updated);
    });
}

/// Phase 5: Spawns an async task to fetch `/youtubei/v1/next` Radio recommendations when the
/// current queue reaches the end, appending the new tracks and seamlessly starting the next song.
fn spawn_radio_continuation(app: AppHandle, seed_video_id: String) {
    let auth_opt = current_auth_snapshot(&app);
    tauri::async_runtime::spawn(async move {
        let radio_tracks = fetch_radio_up_next(&seed_video_id, auth_opt.as_ref())
            .await
            .unwrap_or_else(|_| default_queue_tracks());

        let Some(container) = app.try_state::<AppContainer>() else {
            return;
        };

        let mut updated_queue = Vec::new();
        let mut next_play_info: Option<(PlayerState, String, usize, f64)> = None;

        {
            if let Ok(mut q_guard) = container.queue.lock() {
                let first_new_index = q_guard.len();
                for track in radio_tracks {
                    if !q_guard
                        .iter()
                        .any(|existing| existing.video_id == track.video_id)
                    {
                        q_guard.push(track);
                    }
                }
                // If all tracks were already in the queue, wrap around or append fallback
                let target_index = if first_new_index < q_guard.len() {
                    first_new_index
                } else {
                    0
                };
                updated_queue = q_guard.clone();

                if let Ok(mut state) = container.player.lock() {
                    state.apply_track_from_queue(&updated_queue, target_index);
                    state.is_loading = true;
                    state.is_playing = true;
                    state.audio_quality_label =
                        "Rádio Autoplay (/youtubei/v1/next) • Carregando...".to_string();
                    next_play_info = Some((
                        state.clone(),
                        state.video_id.clone(),
                        state.queue_index,
                        state.duration_secs,
                    ));
                }
            }
        }

        if !updated_queue.is_empty() {
            broadcast_queue_change(&app, &updated_queue);
        }

        if let Some((updated_state, vid, idx, dur)) = next_play_info {
            broadcast_state_change(&app, &updated_state);
            spawn_track_loader(app, vid, idx, dur, false);
        }
    });
}

/// Starts a lightweight background thread that ticks every 500ms to advance `position_secs`
/// while audio is playing, automatically handling RepeatOne, RepeatAll, Queue Advance, or
/// Infinite Radio Autoplay (`/youtubei/v1/next`) when a song finishes.
/// Avoids DBus / System Tray spam on normal 500ms ticks and suppresses IPC emission when the
/// main WebView window is destroyed in Zero-WebView background mode!
fn start_progress_ticker(app: AppHandle) {
    std::thread::Builder::new()
        .name("ytm-progress-ticker".to_string())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(500));

            let Some(container) = app.try_state::<AppContainer>() else {
                continue;
            };

            let mut auto_advance_info: Option<(String, usize, f64)> = None;
            let mut trigger_radio_for_video: Option<String> = None;
            let mut updated_state: Option<PlayerState> = None;
            let mut track_transitioned = false;

            {
                let queue_snapshot = container
                    .queue
                    .lock()
                    .map(|q| q.clone())
                    .unwrap_or_default();

                if let Ok(mut state) = container.player.lock() {
                    if state.is_playing && !state.is_loading {
                        state.position_secs += 0.5;

                        if state.duration_secs > 0.0 && state.position_secs >= state.duration_secs {
                            let queue_len = queue_snapshot.len().max(1);
                            match state.repeat_mode {
                                RepeatMode::One => {
                                    // Replay the exact same song already in RAM
                                    state.position_secs = 0.0;
                                    container.audio_engine.send(AudioCommand::Seek(0.0));
                                }
                                RepeatMode::All => {
                                    let next_idx = (state.queue_index + 1) % queue_len;
                                    state.apply_track_from_queue(&queue_snapshot, next_idx);
                                    state.is_loading = true;
                                    state.is_playing = true;
                                    track_transitioned = true;
                                    auto_advance_info = Some((
                                        state.video_id.clone(),
                                        state.queue_index,
                                        state.duration_secs,
                                    ));
                                }
                                RepeatMode::Off => {
                                    if state.queue_index + 1 < queue_len {
                                        let next_idx = state.queue_index + 1;
                                        state.apply_track_from_queue(&queue_snapshot, next_idx);
                                        state.is_loading = true;
                                        state.is_playing = true;
                                        track_transitioned = true;
                                        auto_advance_info = Some((
                                            state.video_id.clone(),
                                            state.queue_index,
                                            state.duration_secs,
                                        ));
                                    } else if state.autoplay_radio {
                                        // Queue finished and Autoplay Radio is enabled!
                                        state.position_secs = 0.0;
                                        state.is_loading = true;
                                        state.is_playing = true;
                                        state.audio_quality_label =
                                            "Buscando Rádio Infinita (/youtubei/v1/next)..."
                                                .to_string();
                                        track_transitioned = true;
                                        trigger_radio_for_video = Some(state.video_id.clone());
                                    } else {
                                        // End of queue with Repeat Off and Radio Off
                                        state.position_secs = 0.0;
                                        state.is_playing = false;
                                        track_transitioned = true;
                                        container
                                            .audio_engine
                                            .send(AudioCommand::SetPlaying(false));
                                    }
                                }
                            }
                        }

                        updated_state = Some(state.clone());
                    }
                }
            }

            if let Some((vid, idx, dur)) = auto_advance_info {
                spawn_track_loader(app.clone(), vid, idx, dur, false);
            }

            if let Some(seed_vid) = trigger_radio_for_video {
                spawn_radio_continuation(app.clone(), seed_vid);
            }

            if let Some(state_snapshot) = updated_state {
                if track_transitioned {
                    // Track finished / auto-advanced: update tray text (if changed) & WebView (if open)
                    broadcast_state_change(&app, &state_snapshot);
                } else if app.get_webview_window("main").is_some() {
                    // Normal 500ms progress tick: only emit IPC event if main window is open; never touch DBus tray!
                    let _ = app.emit("player-state-changed", &state_snapshot);
                }
            }
        })
        .expect("failed to spawn progress ticker thread");
}

/// Focuses the existing main window or recreates it from scratch if it was destroyed
/// when the user closed it ('X') to free WebView RAM.
pub fn show_or_recreate_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    } else if let Ok(window) =
        WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("YouTube Music (Low-RAM)")
            .inner_size(1100.0, 720.0)
            .min_inner_size(800.0, 550.0)
            .resizable(true)
            .build()
    {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn get_player_state(container: State<'_, AppContainer>) -> PlayerState {
    container
        .player
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn toggle_play(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let mut needs_initial_load: Option<(String, usize, f64)> = None;

    let updated = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        if state.buffer_size_mb <= 0.0 && !state.is_loading {
            state.is_loading = true;
            state.is_playing = true;
            state.error_message = None;
            needs_initial_load = Some((
                state.video_id.clone(),
                state.queue_index,
                state.duration_secs,
            ));
        } else {
            state.is_playing = !state.is_playing;
            container
                .audio_engine
                .send(AudioCommand::SetPlaying(state.is_playing));
        }
        state.clone()
    };

    if let Some((vid, idx, dur)) = needs_initial_load {
        spawn_track_loader(app.clone(), vid, idx, dur, false);
    }

    broadcast_state_change(&app, &updated);
    updated
}

#[tauri::command]
fn next_track(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let queue_snapshot = container
        .queue
        .lock()
        .map(|q| q.clone())
        .unwrap_or_default();

    let mut trigger_radio_vid: Option<String> = None;
    let mut track_to_load: Option<(String, usize, f64)> = None;

    let updated = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        let len = queue_snapshot.len().max(1);
        if state.queue_index + 1 < len {
            let next_idx = state.queue_index + 1;
            state.apply_track_from_queue(&queue_snapshot, next_idx);
            state.is_loading = true;
            state.is_playing = true;
            track_to_load = Some((
                state.video_id.clone(),
                state.queue_index,
                state.duration_secs,
            ));
        } else if state.autoplay_radio && state.repeat_mode == RepeatMode::Off {
            state.is_loading = true;
            state.is_playing = true;
            state.audio_quality_label =
                "Buscando Rádio Infinita (/youtubei/v1/next)...".to_string();
            trigger_radio_vid = Some(state.video_id.clone());
        } else {
            let next_idx = (state.queue_index + 1) % len;
            state.apply_track_from_queue(&queue_snapshot, next_idx);
            state.is_loading = true;
            state.is_playing = true;
            track_to_load = Some((
                state.video_id.clone(),
                state.queue_index,
                state.duration_secs,
            ));
        }
        state.clone()
    };

    broadcast_state_change(&app, &updated);

    if let Some(seed_vid) = trigger_radio_vid {
        spawn_radio_continuation(app, seed_vid);
    } else if let Some((vid, idx, dur)) = track_to_load {
        spawn_track_loader(app, vid, idx, dur, false);
    }

    updated
}

#[tauri::command]
fn prev_track(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let queue_snapshot = container
        .queue
        .lock()
        .map(|q| q.clone())
        .unwrap_or_default();
    let (updated, vid, idx, dur) = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        let len = queue_snapshot.len().max(1);
        let prev_idx = (state.queue_index + len - 1) % len;
        state.apply_track_from_queue(&queue_snapshot, prev_idx);
        state.is_loading = true;
        state.is_playing = true;
        (
            state.clone(),
            state.video_id.clone(),
            state.queue_index,
            state.duration_secs,
        )
    };
    broadcast_state_change(&app, &updated);
    spawn_track_loader(app, vid, idx, dur, false);
    updated
}

#[tauri::command]
fn select_demo_track(
    app: AppHandle,
    container: State<'_, AppContainer>,
    index: usize,
) -> PlayerState {
    let demo_queue = default_queue_tracks();
    if let Ok(mut q_guard) = container.queue.lock() {
        *q_guard = demo_queue.clone();
    }

    let (updated, vid, idx, dur) = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.apply_track_from_queue(&demo_queue, index);
        state.is_loading = true;
        state.is_playing = true;
        (
            state.clone(),
            state.video_id.clone(),
            state.queue_index,
            state.duration_secs,
        )
    };
    broadcast_queue_change(&app, &demo_queue);
    broadcast_state_change(&app, &updated);
    spawn_track_loader(app, vid, idx, dur, false);
    updated
}

#[tauri::command]
fn play_track_list(
    app: AppHandle,
    container: State<'_, AppContainer>,
    tracks: Vec<TrackItem>,
    start_index: usize,
) -> PlayerState {
    let active_queue = if tracks.is_empty() {
        default_queue_tracks()
    } else {
        tracks
    };

    if let Ok(mut q_guard) = container.queue.lock() {
        *q_guard = active_queue.clone();
    }

    let (updated, vid, idx, dur) = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.apply_track_from_queue(&active_queue, start_index);
        state.is_loading = true;
        state.is_playing = true;
        (
            state.clone(),
            state.video_id.clone(),
            state.queue_index,
            state.duration_secs,
        )
    };

    broadcast_queue_change(&app, &active_queue);
    broadcast_state_change(&app, &updated);
    spawn_track_loader(app, vid, idx, dur, false);
    updated
}

#[tauri::command]
fn set_volume(app: AppHandle, container: State<'_, AppContainer>, volume: f32) -> PlayerState {
    let updated = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.volume = volume.clamp(0.0, 1.0);
        container
            .audio_engine
            .send(AudioCommand::SetVolume(state.volume));
        state.clone()
    };
    broadcast_state_change(&app, &updated);
    updated
}

#[tauri::command]
fn seek_to(app: AppHandle, container: State<'_, AppContainer>, position_secs: f64) -> PlayerState {
    let updated = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        let max_dur = state.duration_secs.max(1.0);
        let clamped = position_secs.clamp(0.0, max_dur);
        state.position_secs = clamped;
        container.audio_engine.send(AudioCommand::Seek(clamped));
        state.clone()
    };
    broadcast_state_change(&app, &updated);
    updated
}

#[tauri::command]
fn play_youtube_video(
    app: AppHandle,
    container: State<'_, AppContainer>,
    video_id: String,
    title: Option<String>,
    artist: Option<String>,
) -> PlayerState {
    let clean_id = parse_youtube_video_id(&video_id);
    let has_custom_title = title.as_ref().is_some_and(|t| !t.trim().is_empty());

    let (updated, idx, dur) = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.video_id = clean_id.clone();
        state.current_track_title = title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| format!("YouTube Stream ({})", clean_id));
        state.current_track_artist = artist
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| "YouTube Music Direct Stream".to_string());
        state.current_thumbnail_url = format!("https://i.ytimg.com/vi/{clean_id}/hqdefault.jpg");
        state.position_secs = 0.0;
        state.is_loading = true;
        state.is_playing = true;
        state.error_message = None;
        (state.clone(), state.queue_index, state.duration_secs)
    };

    broadcast_state_change(&app, &updated);
    spawn_track_loader(app, clean_id, idx, dur, !has_custom_title);
    updated
}

// ============================================================================
// Phase 4 & 5 Commands: Queue Drawer, Shuffle, Repeat, Radio Autoplay, Low-RAM Images
// ============================================================================

#[tauri::command]
fn get_queue(container: State<'_, AppContainer>) -> Vec<TrackItem> {
    container
        .queue
        .lock()
        .map(|q| q.clone())
        .unwrap_or_else(|_| default_queue_tracks())
}

#[tauri::command]
fn play_queue_index(
    app: AppHandle,
    container: State<'_, AppContainer>,
    index: usize,
) -> PlayerState {
    let queue_snapshot = container
        .queue
        .lock()
        .map(|q| q.clone())
        .unwrap_or_else(|_| default_queue_tracks());

    let (updated, vid, idx, dur) = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.apply_track_from_queue(&queue_snapshot, index);
        state.is_loading = true;
        state.is_playing = true;
        (
            state.clone(),
            state.video_id.clone(),
            state.queue_index,
            state.duration_secs,
        )
    };

    broadcast_state_change(&app, &updated);
    spawn_track_loader(app, vid, idx, dur, false);
    updated
}

#[tauri::command]
fn remove_from_queue(
    app: AppHandle,
    container: State<'_, AppContainer>,
    index: usize,
) -> Vec<TrackItem> {
    let mut updated_queue = Vec::new();
    let mut updated_state: Option<PlayerState> = None;

    if let Ok(mut q_guard) = container.queue.lock() {
        if index < q_guard.len() && q_guard.len() > 1 {
            q_guard.remove(index);
            if let Ok(mut state) = container.player.lock() {
                if index < state.queue_index {
                    state.queue_index = state.queue_index.saturating_sub(1);
                } else if state.queue_index >= q_guard.len() {
                    state.queue_index = q_guard.len().saturating_sub(1);
                }
                state.queue_len = q_guard.len();
                updated_state = Some(state.clone());
            }
        }
        updated_queue = q_guard.clone();
    }

    broadcast_queue_change(&app, &updated_queue);
    if let Some(st) = updated_state {
        broadcast_state_change(&app, &st);
    }
    updated_queue
}

#[tauri::command]
fn clear_queue(app: AppHandle, container: State<'_, AppContainer>) -> Vec<TrackItem> {
    let mut updated_queue = Vec::new();
    let mut updated_state: Option<PlayerState> = None;

    if let Ok(mut q_guard) = container.queue.lock() {
        if let Ok(mut state) = container.player.lock() {
            if let Some(current_item) = q_guard.get(state.queue_index).cloned() {
                q_guard.clear();
                q_guard.push(current_item);
                state.queue_index = 0;
                state.queue_len = 1;
            } else {
                *q_guard = default_queue_tracks();
                state.queue_index = 0;
                state.queue_len = q_guard.len();
            }
            updated_state = Some(state.clone());
        }
        updated_queue = q_guard.clone();
    }

    broadcast_queue_change(&app, &updated_queue);
    if let Some(st) = updated_state {
        broadcast_state_change(&app, &st);
    }
    updated_queue
}

#[tauri::command]
fn shuffle_queue(app: AppHandle, container: State<'_, AppContainer>) -> Vec<TrackItem> {
    let mut updated_queue = Vec::new();
    let mut updated_state: Option<PlayerState> = None;

    if let Ok(mut q_guard) = container.queue.lock() {
        if q_guard.len() > 2 {
            let current_idx = container
                .player
                .lock()
                .map(|s| s.queue_index.min(q_guard.len() - 1))
                .unwrap_or(0);

            // Keep currently playing track at index 0 so playback is not interrupted
            q_guard.swap(0, current_idx);

            // Lightweight xorshift64* PRNG seeded from system nanoseconds for Fisher-Yates shuffle
            let mut seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                ^ 0xBF58_476D_1CE4_E5B9;

            let len = q_guard.len();
            for i in (1..len).rev() {
                seed ^= seed >> 12;
                seed ^= seed << 25;
                seed ^= seed >> 27;
                let rand_val = seed.wrapping_mul(0x2545_F491_4F6C_DD1D);
                let j = 1 + (rand_val as usize % i);
                q_guard.swap(i, j);
            }

            if let Ok(mut state) = container.player.lock() {
                state.queue_index = 0;
                state.queue_len = q_guard.len();
                updated_state = Some(state.clone());
            }
        }
        updated_queue = q_guard.clone();
    }

    broadcast_queue_change(&app, &updated_queue);
    if let Some(st) = updated_state {
        broadcast_state_change(&app, &st);
    }
    updated_queue
}

#[tauri::command]
fn cycle_repeat_mode(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let updated = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.repeat_mode = state.repeat_mode.cycle();
        state.clone()
    };
    broadcast_state_change(&app, &updated);
    updated
}

#[tauri::command]
fn toggle_autoplay_radio(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let updated = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.autoplay_radio = !state.autoplay_radio;
        state.clone()
    };
    broadcast_state_change(&app, &updated);
    updated
}

#[tauri::command]
fn toggle_low_ram_images(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let updated = {
        let mut state = container.player.lock().expect("player mutex poisoned");
        state.low_ram_no_images = !state.low_ram_no_images;
        state.clone()
    };
    broadcast_state_change(&app, &updated);
    updated
}

// ============================================================================
// Phase 3 Commands: Google Auth (`SAPISIDHASH`) & Innertube API
// ============================================================================

#[tauri::command]
fn get_auth_status(container: State<'_, AppContainer>) -> AuthStatus {
    container
        .auth
        .lock()
        .map(|s| s.to_status())
        .unwrap_or_default()
}

#[tauri::command]
fn open_login_window(app: AppHandle) -> Result<(), String> {
    open_google_login_window(&app)
}

#[tauri::command]
fn set_manual_cookie(
    app: AppHandle,
    container: State<'_, AppContainer>,
    cookie_str: String,
) -> Result<AuthStatus, String> {
    let session = build_session_from_cookie_str(&cookie_str, "manual")?;
    save_session_to_disk(&session)?;
    let status = session.to_status();

    if let Ok(mut guard) = container.auth.lock() {
        *guard = session;
    }

    if let Some(login_win) = app.get_webview_window("google-login") {
        let _ = login_win.close();
    }

    let _ = app.emit("auth-state-changed", &status);
    Ok(status)
}

#[tauri::command]
fn logout_auth(app: AppHandle, container: State<'_, AppContainer>) -> AuthStatus {
    let _ = clear_session_on_disk();
    let empty_session = AuthSession::default();
    let status = empty_session.to_status();

    if let Ok(mut guard) = container.auth.lock() {
        *guard = empty_session;
    }

    if let Some(login_win) = app.get_webview_window("google-login") {
        let _ = login_win.close();
    }

    let _ = app.emit("auth-state-changed", &status);
    status
}

#[tauri::command]
async fn get_home_feed(container: State<'_, AppContainer>) -> Result<Vec<HomeSection>, String> {
    let auth_opt = container
        .auth
        .lock()
        .ok()
        .filter(|a| a.is_authenticated)
        .map(|a| a.clone());
    fetch_home(auth_opt.as_ref()).await
}

#[tauri::command]
async fn search_youtube_music(
    container: State<'_, AppContainer>,
    query: String,
) -> Result<Vec<TrackItem>, String> {
    let auth_opt = container
        .auth
        .lock()
        .ok()
        .filter(|a| a.is_authenticated)
        .map(|a| a.clone());
    search_music(&query, auth_opt.as_ref()).await
}

#[tauri::command]
async fn get_liked_music(container: State<'_, AppContainer>) -> Result<Vec<TrackItem>, String> {
    let auth_opt = container
        .auth
        .lock()
        .ok()
        .filter(|a| a.is_authenticated)
        .map(|a| a.clone());
    fetch_liked_music(auth_opt.as_ref()).await
}

#[tauri::command]
async fn get_user_playlists(
    container: State<'_, AppContainer>,
) -> Result<Vec<PlaylistItem>, String> {
    let auth_opt = container
        .auth
        .lock()
        .ok()
        .filter(|a| a.is_authenticated)
        .map(|a| a.clone());
    fetch_user_playlists(auth_opt.as_ref()).await
}

#[tauri::command]
async fn get_playlist_tracks(
    container: State<'_, AppContainer>,
    playlist_id: String,
) -> Result<Vec<TrackItem>, String> {
    let auth_opt = container
        .auth
        .lock()
        .ok()
        .filter(|a| a.is_authenticated)
        .map(|a| a.clone());
    fetch_playlist_tracks(&playlist_id, auth_opt.as_ref()).await
}

fn setup_system_tray(app: &AppHandle) -> tauri::Result<()> {
    let initial_status_text = {
        let container = app.state::<AppContainer>();
        let state = container.player.lock().expect("player mutex poisoned");
        let text = state.format_tray_status();
        if let Ok(mut last_text) = container.last_tray_text.lock() {
            *last_text = text.clone();
        }
        text
    };

    let status_item = MenuItem::with_id(app, "status", initial_status_text, false, None::<&str>)?;
    let play_pause_item = MenuItem::with_id(app, "play_pause", "Play / Pause", true, None::<&str>)?;
    let next_item = MenuItem::with_id(app, "next", "Próxima Música", true, None::<&str>)?;
    let prev_item = MenuItem::with_id(app, "prev", "Música Anterior", true, None::<&str>)?;
    let shuffle_item = MenuItem::with_id(
        app,
        "shuffle",
        "Embaralhar Fila (Shuffle)",
        true,
        None::<&str>,
    )?;
    let radio_item = MenuItem::with_id(
        app,
        "toggle_radio",
        "Alternar Rádio Autoplay Infinita",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let open_window_item =
        MenuItem::with_id(app, "open_window", "Abrir Janela (UI)", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Sair Completamente", true, None::<&str>)?;

    {
        let container = app.state::<AppContainer>();
        let mut tray_item_guard = container
            .tray_status_item
            .lock()
            .expect("tray item mutex poisoned");
        *tray_item_guard = Some(status_item.clone());
    }

    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &play_pause_item,
            &next_item,
            &prev_item,
            &shuffle_item,
            &radio_item,
            &separator,
            &open_window_item,
            &quit_item,
        ],
    )?;

    let mut tray_builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("YouTube Music (Low-RAM)")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "play_pause" => {
                let container = app.state::<AppContainer>();
                let _ = toggle_play(app.clone(), container);
            }
            "next" => {
                let container = app.state::<AppContainer>();
                let _ = next_track(app.clone(), container);
            }
            "prev" => {
                let container = app.state::<AppContainer>();
                let _ = prev_track(app.clone(), container);
            }
            "shuffle" => {
                let container = app.state::<AppContainer>();
                let _ = shuffle_queue(app.clone(), container);
            }
            "toggle_radio" => {
                let container = app.state::<AppContainer>();
                let _ = toggle_autoplay_radio(app.clone(), container);
            }
            "open_window" => {
                show_or_recreate_main_window(app);
            }
            "quit" => {
                let container = app.state::<AppContainer>();
                container.download_generation.fetch_add(1, Ordering::SeqCst);
                container.audio_engine.send(AudioCommand::Stop);
                container.quit_requested.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_or_recreate_main_window(tray.app_handle());
            }
        });

    if let Some(default_icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(default_icon.clone());
    }

    tray_builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(AppContainer::default())
        .setup(|app| {
            setup_system_tray(app.handle())?;
            start_progress_ticker(app.handle().clone());
            // Ensure yt-dlp binary is ready in background at startup
            tauri::async_runtime::spawn(async {
                let _ = audio::ensure_yt_dlp_binary().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_player_state,
            toggle_play,
            next_track,
            prev_track,
            select_demo_track,
            play_track_list,
            set_volume,
            seek_to,
            play_youtube_video,
            get_queue,
            play_queue_index,
            remove_from_queue,
            clear_queue,
            shuffle_queue,
            cycle_repeat_mode,
            toggle_autoplay_radio,
            toggle_low_ram_images,
            get_auth_status,
            open_login_window,
            set_manual_cookie,
            logout_auth,
            get_home_feed,
            search_youtube_music,
            get_liked_music,
            get_user_playlists,
            get_playlist_tracks,
        ])
        .build(tauri::generate_context!())
        .expect("error while building YouTube Music (Low-RAM) Tauri application");

    // Zero-WebView Background Lifecycle:
    // When the user closes the window ('X'), the WebView is completely destroyed to free RAM,
    // triggering RunEvent::ExitRequested when the last window closes. We prevent process exit
    // unless the user explicitly clicked "Sair Completamente" in the System Tray menu.
    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let quit_requested = app_handle
                .try_state::<AppContainer>()
                .map(|c| c.quit_requested.load(Ordering::SeqCst))
                .unwrap_or(false);

            if !quit_requested {
                api.prevent_exit();
            }
        }
    });
}
