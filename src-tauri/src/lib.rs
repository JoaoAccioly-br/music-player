pub mod audio;
pub mod auth;
pub mod innertube;
pub mod persistence;

use audio::{
    extract_youtube_audio_stream_with_cancel, parse_youtube_video_id, AudioCommand, AudioEngine,
    ProgressiveAudioBuffer,
};
use auth::{
    build_session_from_cookie_str, clear_session_on_disk, load_session_from_disk,
    open_google_login_window, save_session_to_disk, AuthSession, AuthStatus,
};
use innertube::{
    fetch_home, fetch_liked_music, fetch_playlist_tracks, fetch_radio_up_next, fetch_track_lyrics,
    fetch_user_playlists, search_music_filtered, set_track_like_status, HomeSection,
    LyricsResponse, PlaylistItem, TrackItem,
};
use persistence::{load_persisted_session, save_persisted_session, PersistedSession};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, Wry,
};

const MAX_RADIO_QUEUE_LEN: usize = 300;

/// Curated initial list of real YouTube Music catalog tracks with verified YouTube video IDs and durations:
/// `(video_id, title, artist, album, duration_secs)`
const INITIAL_CATALOG_TRACKS: &[(&str, &str, &str, &str, f64)] = &[
    ("MV_3Dpw-BRY", "Nightcall", "Kavinsky", "OutRun", 258.0),
    (
        "fJ9rUzIMcZQ",
        "Bohemian Rhapsody",
        "Queen",
        "A Night at the Opera",
        355.0,
    ),
    (
        "4NRXx6U8ABQ",
        "Blinding Lights",
        "The Weeknd",
        "After Hours",
        200.0,
    ),
    (
        "FGBhQbmPwH8",
        "One More Time",
        "Daft Punk",
        "Discovery",
        320.0,
    ),
];

fn default_queue_tracks() -> Vec<TrackItem> {
    INITIAL_CATALOG_TRACKS
        .iter()
        .map(|(vid, title, artist, album, dur)| {
            let mins = (*dur as u64) / 60;
            let secs = (*dur as u64) % 60;
            TrackItem {
                video_id: (*vid).to_string(),
                title: (*title).to_string(),
                artist: (*artist).to_string(),
                album: (*album).to_string(),
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

    pub fn as_str(self) -> &'static str {
        match self {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::One => "one",
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
        let (video_id, title, artist, _album, duration_secs) = INITIAL_CATALOG_TRACKS[0];
        Self {
            is_playing: false,
            current_track_title: title.to_string(),
            current_track_artist: artist.to_string(),
            current_thumbnail_url: format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"),
            volume: 0.8,
            queue_index: 0,
            queue_len: INITIAL_CATALOG_TRACKS.len(),
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
        self.buffer_size_mb = 0.0;

        if queue.is_empty() {
            let safe_index = index % INITIAL_CATALOG_TRACKS.len();
            let (video_id, title, artist, _album, duration_secs) =
                INITIAL_CATALOG_TRACKS[safe_index];
            self.queue_index = safe_index;
            self.queue_len = INITIAL_CATALOG_TRACKS.len();
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
/// dedicated native audio engine thread, active progressive audio buffer, tray status menu item handle,
/// deduplicated tray text, download generation counter, stream cancellation flags,
/// prefetched next-track RAM buffer, prefetch generation counter, and exit flag.
///
/// Global lock ordering invariant: ALWAYS acquire `queue` BEFORE `player` (never `player` then `queue`).
pub struct AppContainer {
    pub player: Mutex<PlayerState>,
    pub queue: Mutex<Vec<TrackItem>>,
    pub liked_tracks: Mutex<Vec<TrackItem>>,
    pub auth: Mutex<AuthSession>,
    pub audio_engine: AudioEngine,
    pub active_buffer: Mutex<Option<ProgressiveAudioBuffer>>,
    pub tray_status_item: Mutex<Option<MenuItem<Wry>>>,
    pub last_tray_text: Mutex<String>,
    pub download_generation: Arc<AtomicU64>,
    pub prefetched_audio: Mutex<Option<(String, audio::ExtractedAudio)>>,
    pub prefetch_generation: Arc<AtomicU64>,
    pub active_stream_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub prefetch_stream_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub quit_requested: AtomicBool,
}

impl AppContainer {
    /// Locks `player` safely, recovering from lock poisoning without panicking.
    pub fn lock_player(&self) -> MutexGuard<'_, PlayerState> {
        self.player.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Locks `queue` safely, recovering from lock poisoning without panicking.
    pub fn lock_queue(&self) -> MutexGuard<'_, Vec<TrackItem>> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for AppContainer {
    fn default() -> Self {
        let initial_auth = load_session_from_disk();
        let audio_engine = AudioEngine::new();
        let mut player_state = PlayerState::default();
        let mut initial_queue = default_queue_tracks();
        let mut initial_liked_tracks = Vec::new();

        if let Some(persisted) = load_persisted_session() {
            let vol = persisted.volume.clamp(0.0, 1.0);
            player_state.volume = vol;
            audio_engine.send(AudioCommand::SetVolume(vol));

            player_state.repeat_mode = match persisted.repeat_mode.as_str() {
                "all" => RepeatMode::All,
                "one" => RepeatMode::One,
                _ => RepeatMode::Off,
            };
            player_state.autoplay_radio = persisted.autoplay_radio;
            player_state.low_ram_no_images = persisted.low_ram_no_images;

            if !persisted.queue.is_empty() {
                initial_queue = persisted.queue;
            }
            initial_liked_tracks = persisted.liked_tracks;
            player_state.apply_track_from_queue(&initial_queue, persisted.queue_index);
        }

        Self {
            player: Mutex::new(player_state),
            queue: Mutex::new(initial_queue),
            liked_tracks: Mutex::new(initial_liked_tracks),
            auth: Mutex::new(initial_auth),
            audio_engine,
            active_buffer: Mutex::new(None),
            tray_status_item: Mutex::new(None),
            last_tray_text: Mutex::new(String::new()),
            download_generation: Arc::new(AtomicU64::new(0)),
            prefetched_audio: Mutex::new(None),
            prefetch_generation: Arc::new(AtomicU64::new(0)),
            active_stream_cancel: Mutex::new(None),
            prefetch_stream_cancel: Mutex::new(None),
            quit_requested: AtomicBool::new(false),
        }
    }
}

/// Atomically cancels any previous `Arc<AtomicBool>` stream flag stored in `slot`
/// (unless it points to the exact same allocation as `next`) and installs `next`.
fn cancel_and_replace(slot: &Mutex<Option<Arc<AtomicBool>>>, next: Option<Arc<AtomicBool>>) {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(prev) = guard.take() {
        let is_same = next.as_ref().is_some_and(|n| Arc::ptr_eq(&prev, n));
        if !is_same {
            prev.store(true, Ordering::SeqCst);
        }
    }
    *guard = next;
}

/// Deduplicates `radio_tracks` by `video_id`, appends them to `queue`, and caps `queue` to at most
/// `max_len` entries using a sliding window that drains oldest already-played tracks from the front
/// (preserving up to 30 history items before `state.queue_index` when possible) while keeping
/// `state.queue_index` valid. Returns the count of newly added tracks remaining in `queue`.
pub fn append_radio_tracks_capped(
    queue: &mut Vec<TrackItem>,
    state: &mut PlayerState,
    radio_tracks: Vec<TrackItem>,
    max_len: usize,
) -> usize {
    let mut existing_ids: HashSet<String> = queue.iter().map(|t| t.video_id.clone()).collect();
    let mut added = 0usize;

    for track in radio_tracks {
        let vid = track.video_id.trim();
        if !vid.is_empty() && existing_ids.insert(vid.to_string()) {
            queue.push(track);
            added += 1;
        }
    }

    if max_len > 0 && queue.len() > max_len {
        let excess = queue.len() - max_len;
        let max_drainable = state.queue_index.saturating_sub(30);
        let drained = excess.min(max_drainable);
        if drained > 0 {
            queue.drain(0..drained);
            state.queue_index -= drained;
        }
        if queue.len() > max_len {
            let truncated = queue.len() - max_len;
            queue.truncate(max_len);
            added = added.saturating_sub(truncated);
        }
    }

    if !queue.is_empty() && state.queue_index >= queue.len() {
        state.queue_index = queue.len() - 1;
    }
    state.queue_len = queue.len();
    added
}

/// Captures a consistent snapshot of the current session using strict `queue -> player` lock order.
fn snapshot_persisted_session(app: &AppHandle) -> Option<PersistedSession> {
    let container = app.try_state::<AppContainer>()?;
    let (queue_snapshot, state_snapshot) = {
        let queue = container.lock_queue();
        let state = container.lock_player();
        (queue.clone(), state.clone())
    };
    let liked_snapshot = container
        .liked_tracks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Some(PersistedSession {
        volume: state_snapshot.volume,
        repeat_mode: state_snapshot.repeat_mode.as_str().to_string(),
        autoplay_radio: state_snapshot.autoplay_radio,
        low_ram_no_images: state_snapshot.low_ram_no_images,
        theme: "dark".to_string(),
        queue: queue_snapshot,
        queue_index: state_snapshot.queue_index,
        liked_tracks: liked_snapshot,
    })
}

/// Persists the current player state, queue, and liked tracks to `<app_data_dir>/player_session.json`
/// off the Tokio worker pool via `spawn_blocking`.
fn persist_current_session(app: &AppHandle) {
    if let Some(session) = snapshot_persisted_session(app) {
        tauri::async_runtime::spawn_blocking(move || {
            save_persisted_session(&session);
        });
    }
}

/// Determines the `video_id` of the next track to prefetch into RAM based on the current queue,
/// active index, and repeat mode.
pub fn determine_next_video_id(
    queue: &[TrackItem],
    current_index: usize,
    repeat_mode: RepeatMode,
) -> Option<String> {
    if queue.is_empty() {
        return None;
    }
    if repeat_mode == RepeatMode::One {
        // Same song is already in the AudioEngine buffer
        return None;
    }
    if current_index + 1 < queue.len() {
        return Some(queue[current_index + 1].video_id.clone());
    }
    if repeat_mode == RepeatMode::All {
        return Some(queue[0].video_id.clone());
    }
    None
}

/// Synchronizes state changes with both the native System Tray menu and the WebView (if open).
/// Deduplicates `status_item.set_text(...)` via `last_tray_text` so Linux DBus / AppIndicator /
/// GNOME Shell / Xwayland are never spammed when the tray text hasn't changed!
fn broadcast_state_change(app: &AppHandle, state: &PlayerState) {
    if let Some(container) = app.try_state::<AppContainer>() {
        let new_tray_text = state.format_tray_status();
        let should_update_tray = {
            let mut last_text = container
                .last_tray_text
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *last_text != new_tray_text {
                *last_text = new_tray_text.clone();
                true
            } else {
                false
            }
        };

        if should_update_tray {
            let guard = container
                .tray_status_item
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(status_item) = guard.as_ref() {
                let _ = status_item.set_text(new_tray_text);
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
    let guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_authenticated && !guard.sapisid.is_empty() {
        Some(guard.clone())
    } else {
        None
    }
}

/// Spawns a background task to pre-load (prefetch) the next song in the queue into RAM
/// (`container.prefetched_audio`). If the user is on the last track of the queue with
/// `RepeatMode::Off` and `autoplay_radio == true`, proactively fetches Radio recommendations
/// (`/youtubei/v1/next`), appends them to the queue (capped at 300), and prefetches the first newly added track!
fn spawn_prefetch_next_track(app: AppHandle) {
    let Some(container) = app.try_state::<AppContainer>() else {
        return;
    };

    let (queue_snapshot, current_index, repeat_mode, autoplay_radio, current_video_id) = {
        let queue = container.lock_queue();
        let state = container.lock_player();
        (
            queue.clone(),
            state.queue_index,
            state.repeat_mode,
            state.autoplay_radio,
            state.video_id.clone(),
        )
    };

    if let Some(next_vid) = determine_next_video_id(&queue_snapshot, current_index, repeat_mode) {
        let clean_next_vid = parse_youtube_video_id(&next_vid);

        // If `prefetched_audio` already holds this exact `next_vid`, do nothing!
        {
            let guard = container
                .prefetched_audio
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some((ref cached_vid, _)) = *guard {
                if *cached_vid == clean_next_vid || *cached_vid == next_vid {
                    return;
                }
            }
        }

        let auth_opt = current_auth_snapshot(&app);
        let prefetch_gen = container.prefetch_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let prefetch_counter = container.prefetch_generation.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        cancel_and_replace(&container.prefetch_stream_cancel, Some(cancel.clone()));

        tauri::async_runtime::spawn(async move {
            let extraction_res = extract_youtube_audio_stream_with_cancel(
                &clean_next_vid,
                auth_opt.as_ref(),
                Some(cancel.clone()),
            )
            .await;

            if let Ok(extracted) = extraction_res {
                if cancel.load(Ordering::SeqCst)
                    || prefetch_counter.load(Ordering::SeqCst) != prefetch_gen
                {
                    return;
                }
                if let Some(c) = app.try_state::<AppContainer>() {
                    let mut guard = c.prefetched_audio.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = Some((clean_next_vid, extracted));
                }
            }
        });
    } else if !queue_snapshot.is_empty()
        && current_index + 1 >= queue_snapshot.len()
        && repeat_mode == RepeatMode::Off
        && autoplay_radio
    {
        // Last song in the queue with Repeat Off and Autoplay Radio enabled:
        // Proactively fetch radio recommendations, append to queue, and prefetch the first new song!
        let auth_opt = current_auth_snapshot(&app);
        let prefetch_gen = container.prefetch_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let prefetch_counter = container.prefetch_generation.clone();
        cancel_and_replace(&container.prefetch_stream_cancel, None);

        tauri::async_runtime::spawn(async move {
            let Ok(radio_tracks) = fetch_radio_up_next(&current_video_id, auth_opt.as_ref()).await
            else {
                return;
            };

            if prefetch_counter.load(Ordering::SeqCst) != prefetch_gen {
                return;
            }

            let Some(c) = app.try_state::<AppContainer>() else {
                return;
            };

            let mut updated_queue = Vec::new();
            let mut updated_state_opt: Option<PlayerState> = None;
            let mut next_vid_to_prefetch: Option<String> = None;

            {
                let mut q_guard = c.lock_queue();
                let mut state = c.lock_player();
                // Verify the player is still on the last track with radio enabled
                if state.video_id != current_video_id
                    || state.repeat_mode != RepeatMode::Off
                    || !state.autoplay_radio
                {
                    return;
                }

                let added = append_radio_tracks_capped(
                    &mut q_guard,
                    &mut state,
                    radio_tracks,
                    MAX_RADIO_QUEUE_LEN,
                );

                if added > 0 {
                    updated_queue = q_guard.clone();
                    updated_state_opt = Some(state.clone());
                    if state.queue_index + 1 < q_guard.len() {
                        next_vid_to_prefetch =
                            Some(q_guard[state.queue_index + 1].video_id.clone());
                    }
                }
            }

            if !updated_queue.is_empty() {
                broadcast_queue_change(&app, &updated_queue);
                persist_current_session(&app);
            }
            if let Some(st) = updated_state_opt {
                broadcast_state_change(&app, &st);
            }

            if let Some(next_vid) = next_vid_to_prefetch {
                let clean_next_vid = parse_youtube_video_id(&next_vid);

                // Check if already cached
                let already_cached = {
                    let g = c.prefetched_audio.lock().unwrap_or_else(|e| e.into_inner());
                    g.as_ref()
                        .is_some_and(|(vid, _)| *vid == clean_next_vid || *vid == next_vid)
                };
                if already_cached {
                    return;
                }

                if prefetch_counter.load(Ordering::SeqCst) != prefetch_gen {
                    return;
                }

                let cancel = Arc::new(AtomicBool::new(false));
                cancel_and_replace(&c.prefetch_stream_cancel, Some(cancel.clone()));

                let extraction_res = extract_youtube_audio_stream_with_cancel(
                    &clean_next_vid,
                    auth_opt.as_ref(),
                    Some(cancel.clone()),
                )
                .await;

                if let Ok(extracted) = extraction_res {
                    if cancel.load(Ordering::SeqCst)
                        || prefetch_counter.load(Ordering::SeqCst) != prefetch_gen
                    {
                        return;
                    }
                    let mut guard = c.prefetched_audio.lock().unwrap_or_else(|e| e.into_inner());
                    *guard = Some((clean_next_vid, extracted));
                }
            }
        });
    }
}

/// Spawns an asynchronous background task on Tauri's Tokio runtime to extract the real YouTube audio
/// stream (via isolated `~/.local/share/com.ytmusic.rust/bin/yt-dlp` or Innertube) into a RAM
/// `ProgressiveAudioBuffer` and feed it to `AudioEngine`. First checks `prefetched_audio` for an
/// instant RAM Cache Hit (promoting its `cancel_flag` to `active_stream_cancel` so subsequent
/// prefetches never kill the active stream!). On Cache Miss, cancels any in-flight prefetch, clears
/// stale prefetch RAM, and installs a fresh active stream cancellation flag.
fn spawn_track_loader(
    app: AppHandle,
    video_id: String,
    _seed_index: usize,
    fallback_duration: f64,
    allow_metadata_override: bool,
) {
    let clean_video_id = parse_youtube_video_id(&video_id);

    let Some(container) = app.try_state::<AppContainer>() else {
        return;
    };

    // Cancel any previous active track download generation
    let expected_gen = container.download_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let generation_counter = container.download_generation.clone();

    // 1. Check RAM Prefetch Cache before spawning any download!
    let cached_opt = {
        let mut guard = container
            .prefetched_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard
            .as_ref()
            .is_some_and(|(cached_vid, _)| *cached_vid == clean_video_id || *cached_vid == video_id)
        {
            guard.take().map(|(_, audio)| audio)
        } else {
            None
        }
    };

    if let Some(extracted) = cached_opt {
        // RAM Prefetch Cache Hit! Promote prefetch stream to active stream WITHOUT cancelling it,
        // so spawning the next prefetch (N + 2) will NOT kill this active stream (N + 1)!
        {
            let mut prefetch_guard = container
                .prefetch_stream_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *prefetch_guard = None;
        }
        cancel_and_replace(
            &container.active_stream_cancel,
            extracted.cancel_flag.clone(),
        );

        let updated = {
            let mut state = container.lock_player();
            let buffer_mb = (extracted.data.len() as f32) / (1024.0 * 1024.0);
            let current_vol = state.volume;
            let start_paused = !state.is_playing;

            {
                let mut buf_guard = container
                    .active_buffer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                *buf_guard = Some(extracted.data.clone());
            }

            container.audio_engine.send(AudioCommand::LoadAndPlay {
                data: extracted.data,
                volume: current_vol,
                start_paused,
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
            state.audio_quality_label =
                format!("{} • Prefetch RAM Instantâneo", extracted.quality_label);
            state.is_loading = false;
            state.is_playing = !start_paused;
            state.error_message = None;
            state.clone()
        };

        broadcast_state_change(&app, &updated);
        // Immediately trigger background prefetch for the subsequent track (N + 2)!
        spawn_prefetch_next_track(app);
        return;
    }

    // 2. Cache Miss: Cancel any running background prefetch and clear stale prefetch RAM
    container.prefetch_generation.fetch_add(1, Ordering::SeqCst);
    cancel_and_replace(&container.prefetch_stream_cancel, None);
    {
        let mut guard = container
            .prefetched_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    let active_cancel = Arc::new(AtomicBool::new(false));
    cancel_and_replace(&container.active_stream_cancel, Some(active_cancel.clone()));

    let auth_opt = current_auth_snapshot(&app);

    tauri::async_runtime::spawn(async move {
        let extraction_result = extract_youtube_audio_stream_with_cancel(
            &video_id,
            auth_opt.as_ref(),
            Some(active_cancel.clone()),
        )
        .await;

        let Some(container) = app.try_state::<AppContainer>() else {
            return;
        };

        let mut loaded_ok = false;
        let updated = {
            let mut state = container.lock_player();
            // Ensure the user hasn't switched to a different video or newer generation while downloading
            if state.video_id != video_id && state.video_id != clean_video_id {
                return;
            }
            if active_cancel.load(Ordering::SeqCst)
                || generation_counter.load(Ordering::SeqCst) != expected_gen
            {
                return;
            }

            match extraction_result {
                Ok(extracted) => {
                    loaded_ok = true;
                    let buffer_mb = (extracted.data.len() as f32) / (1024.0 * 1024.0);
                    let current_vol = state.volume;
                    let start_paused = !state.is_playing;

                    {
                        let mut buf_guard = container
                            .active_buffer
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        *buf_guard = Some(extracted.data.clone());
                    }

                    container.audio_engine.send(AudioCommand::LoadAndPlay {
                        data: extracted.data,
                        volume: current_vol,
                        start_paused,
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
                    state.is_playing = !start_paused;
                    state.error_message = None;
                }
                Err(err_msg) => {
                    {
                        let mut buf_guard = container
                            .active_buffer
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        *buf_guard = None;
                    }
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

        // Once the current track is loaded and playing, prefetch the next track in the background!
        if loaded_ok {
            spawn_prefetch_next_track(app);
        }
    });
}

/// Phase 5: Spawns an async task to fetch `/youtubei/v1/next` Radio recommendations when the
/// current queue reaches the end, appending the new tracks (capped at 300) and seamlessly starting
/// the next song. Increments `download_generation` upfront to prevent races with concurrent skips.
fn spawn_radio_continuation(app: AppHandle, seed_video_id: String) {
    let auth_opt = current_auth_snapshot(&app);
    let expected_gen_opt = app.try_state::<AppContainer>().map(|c| {
        cancel_and_replace(&c.active_stream_cancel, None);
        c.download_generation.fetch_add(1, Ordering::SeqCst) + 1
    });

    tauri::async_runtime::spawn(async move {
        let radio_tracks = fetch_radio_up_next(&seed_video_id, auth_opt.as_ref())
            .await
            .unwrap_or_else(|_| default_queue_tracks());

        let Some(container) = app.try_state::<AppContainer>() else {
            return;
        };

        if let Some(expected_gen) = expected_gen_opt {
            if container.download_generation.load(Ordering::SeqCst) != expected_gen {
                return;
            }
        }

        // Strict lock order: lock `queue` BEFORE `player` so append + state transition are atomic
        let (updated_queue, next_play_info) = {
            let mut q_guard = container.lock_queue();
            let mut state = container.lock_player();

            if let Some(expected_gen) = expected_gen_opt {
                if container.download_generation.load(Ordering::SeqCst) != expected_gen {
                    return;
                }
            }
            if state.video_id != seed_video_id || !state.is_loading {
                return;
            }

            let added = append_radio_tracks_capped(
                &mut q_guard,
                &mut state,
                radio_tracks,
                MAX_RADIO_QUEUE_LEN,
            );
            let target_idx = if added > 0 { q_guard.len() - added } else { 0 };

            state.apply_track_from_queue(&q_guard, target_idx);
            state.is_loading = true;
            state.is_playing = true;
            state.audio_quality_label =
                "Rádio Autoplay (/youtubei/v1/next) • Carregando...".to_string();

            (
                q_guard.clone(),
                (
                    state.clone(),
                    state.video_id.clone(),
                    state.queue_index,
                    state.duration_secs,
                ),
            )
        };

        if !updated_queue.is_empty() {
            broadcast_queue_change(&app, &updated_queue);
            persist_current_session(&app);
        }

        let (updated_state, vid, idx, dur) = next_play_info;
        broadcast_state_change(&app, &updated_state);
        spawn_track_loader(app, vid, idx, dur, false);
    });
}

/// Starts a lightweight background thread that ticks every 250ms to synchronize real hardware
/// `rodio::Sink` telemetry (`get_position_secs()`, `is_track_finished()`) and progressive buffer
/// growth (`active_buffer`), automatically handling RepeatOne, RepeatAll, Queue Advance, or
/// Infinite Radio Autoplay (`/youtubei/v1/next`) when a song finishes.
/// Strictly obeys `queue -> player` lock order (never locks `queue` while holding `player`) and
/// skips cloning `PlayerState` when the main WebView window is destroyed in Zero-WebView mode!
fn start_progress_ticker(app: AppHandle) {
    std::thread::Builder::new()
        .name("ytm-progress-ticker".to_string())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));

            let Some(container) = app.try_state::<AppContainer>() else {
                continue;
            };

            let has_window = app.get_webview_window("main").is_some();
            let mut auto_advance_info: Option<(String, usize, f64)> = None;
            let mut trigger_radio_for_video: Option<String> = None;
            let mut updated_state: Option<PlayerState> = None;
            let mut track_transitioned = false;
            let mut needs_queue_advance = false;

            {
                let mut state = container.lock_player();
                if let Some(audio_err) = container.audio_engine.take_last_error() {
                    state.is_playing = false;
                    state.is_loading = false;
                    state.error_message = Some(audio_err);
                    state.audio_quality_label =
                        "Erro no dispositivo/decodificador de áudio".to_string();
                    track_transitioned = true;
                    updated_state = Some(state.clone());
                } else if state.is_playing && !state.is_loading {
                    let real_pos = container.audio_engine.get_position_secs();
                    if real_pos > 0.0 {
                        state.position_secs = real_pos;
                    } else {
                        state.position_secs += 0.25;
                    }

                    {
                        let buf_guard = container
                            .active_buffer
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if let Some(ref buf) = *buf_guard {
                            state.buffer_size_mb = (buf.len() as f32) / (1024.0 * 1024.0);
                            if buf.is_finished()
                                && state.audio_quality_label.contains("Streaming Progressivo")
                                && !state.audio_quality_label.contains("Buffer Completo")
                            {
                                state.audio_quality_label =
                                    format!("{} • Buffer Completo", state.audio_quality_label);
                            }
                        }
                    }

                    // Only rely on duration_secs timer fallback when no real hardware Sink is active
                    let track_finished = container.audio_engine.is_track_finished()
                        || (!container.audio_engine.has_active_track()
                            && state.duration_secs > 0.0
                            && state.position_secs >= state.duration_secs);

                    if track_finished {
                        if state.repeat_mode == RepeatMode::One {
                            // Replay the exact same song already in RAM
                            state.position_secs = 0.0;
                            container.audio_engine.send(AudioCommand::Seek(0.0));
                            if has_window {
                                updated_state = Some(state.clone());
                            }
                        } else {
                            // Drop `player` guard first so we acquire `queue` -> `player` in global order
                            needs_queue_advance = true;
                        }
                    } else if has_window {
                        updated_state = Some(state.clone());
                    }
                }
            }

            if needs_queue_advance {
                let queue_guard = container.lock_queue();
                let mut state = container.lock_player();
                if state.is_playing && !state.is_loading {
                    let queue_len = queue_guard.len().max(1);
                    match state.repeat_mode {
                        RepeatMode::One => {
                            state.position_secs = 0.0;
                            container.audio_engine.send(AudioCommand::Seek(0.0));
                            if has_window {
                                updated_state = Some(state.clone());
                            }
                        }
                        RepeatMode::All => {
                            let next_idx = (state.queue_index + 1) % queue_len;
                            state.apply_track_from_queue(&queue_guard, next_idx);
                            state.is_loading = true;
                            state.is_playing = true;
                            track_transitioned = true;
                            auto_advance_info = Some((
                                state.video_id.clone(),
                                state.queue_index,
                                state.duration_secs,
                            ));
                            updated_state = Some(state.clone());
                        }
                        RepeatMode::Off => {
                            if state.queue_index + 1 < queue_len {
                                let next_idx = state.queue_index + 1;
                                state.apply_track_from_queue(&queue_guard, next_idx);
                                state.is_loading = true;
                                state.is_playing = true;
                                track_transitioned = true;
                                auto_advance_info = Some((
                                    state.video_id.clone(),
                                    state.queue_index,
                                    state.duration_secs,
                                ));
                                updated_state = Some(state.clone());
                            } else if state.autoplay_radio {
                                // Queue finished and Autoplay Radio is enabled!
                                state.position_secs = 0.0;
                                state.is_loading = true;
                                state.is_playing = true;
                                state.audio_quality_label =
                                    "Buscando Rádio Infinita (/youtubei/v1/next)...".to_string();
                                track_transitioned = true;
                                trigger_radio_for_video = Some(state.video_id.clone());
                                updated_state = Some(state.clone());
                            } else {
                                // End of queue with Repeat Off and Radio Off
                                state.position_secs = 0.0;
                                state.is_playing = false;
                                track_transitioned = true;
                                container.audio_engine.send(AudioCommand::SetPlaying(false));
                                updated_state = Some(state.clone());
                            }
                        }
                    }
                }
            }

            if let Some(state_snapshot) = updated_state {
                if track_transitioned {
                    // Track finished / auto-advanced: update tray text (if changed), persist session & update WebView
                    broadcast_state_change(&app, &state_snapshot);
                    persist_current_session(&app);
                } else if has_window {
                    // Normal 250ms progress tick: only emit IPC event if main window is open; never touch DBus tray!
                    let _ = app.emit("player-state-changed", &state_snapshot);
                }
            }

            if let Some((vid, idx, dur)) = auto_advance_info {
                spawn_track_loader(app.clone(), vid, idx, dur, false);
            }

            if let Some(seed_vid) = trigger_radio_for_video {
                spawn_radio_continuation(app.clone(), seed_vid);
            }
        })
        .expect("failed to spawn progress ticker thread");
}

/// Focuses the existing main window or recreates it from scratch if it was destroyed
/// when the user closed it ('X') to free WebView RAM.
pub fn show_or_recreate_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

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
    container.lock_player().clone()
}

#[tauri::command]
fn toggle_play(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let mut needs_initial_load: Option<(String, usize, f64)> = None;

    let updated = {
        let mut state = container.lock_player();
        if state.video_id.trim().is_empty() {
            return state.clone();
        }
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
            if state.is_playing && !state.is_loading && container.audio_engine.is_track_finished() {
                state.position_secs = 0.0;
                container.audio_engine.send(AudioCommand::Seek(0.0));
            }
            container
                .audio_engine
                .send(AudioCommand::SetPlaying(state.is_playing));
        }
        state.clone()
    };

    broadcast_state_change(&app, &updated);

    if let Some((vid, idx, dur)) = needs_initial_load {
        spawn_track_loader(app, vid, idx, dur, false);
        return get_player_state(container);
    }

    updated
}

#[tauri::command]
fn next_track(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let queue_snapshot = container.lock_queue().clone();

    let mut trigger_radio_vid: Option<String> = None;
    let mut track_to_load: Option<(String, usize, f64)> = None;

    let updated = {
        let mut state = container.lock_player();
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
    persist_current_session(&app);

    if let Some(seed_vid) = trigger_radio_vid {
        spawn_radio_continuation(app, seed_vid);
    } else if let Some((vid, idx, dur)) = track_to_load {
        spawn_track_loader(app, vid, idx, dur, false);
        return get_player_state(container);
    }

    updated
}

#[tauri::command]
fn prev_track(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let queue_snapshot = container.lock_queue().clone();
    let (updated, vid, idx, dur) = {
        let mut state = container.lock_player();
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
    persist_current_session(&app);
    spawn_track_loader(app, vid, idx, dur, false);
    get_player_state(container)
}

#[tauri::command]
fn play_track_list(
    app: AppHandle,
    container: State<'_, AppContainer>,
    tracks: Vec<TrackItem>,
    start_index: usize,
) -> PlayerState {
    let mut active_queue = if tracks.is_empty() {
        default_queue_tracks()
    } else {
        tracks
    };
    if active_queue.len() > 500 {
        active_queue.truncate(500);
    }

    {
        let mut q_guard = container.lock_queue();
        *q_guard = active_queue.clone();
    }

    let (updated, vid, idx, dur) = {
        let mut state = container.lock_player();
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
    persist_current_session(&app);
    spawn_track_loader(app, vid, idx, dur, false);
    get_player_state(container)
}

#[tauri::command]
fn set_volume(app: AppHandle, container: State<'_, AppContainer>, volume: f32) -> PlayerState {
    let updated = {
        let mut state = container.lock_player();
        state.volume = volume.clamp(0.0, 1.0);
        container
            .audio_engine
            .send(AudioCommand::SetVolume(state.volume));
        state.clone()
    };
    broadcast_state_change(&app, &updated);
    persist_current_session(&app);
    updated
}

#[tauri::command]
fn seek_to(app: AppHandle, container: State<'_, AppContainer>, position_secs: f64) -> PlayerState {
    let updated = {
        let mut state = container.lock_player();
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
fn clear_error(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let updated = {
        let mut state = container.lock_player();
        state.error_message = None;
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
    let custom_title = title.filter(|t| !t.trim().is_empty());
    let custom_artist = artist.filter(|a| !a.trim().is_empty());
    let has_custom_title = custom_title.is_some();
    let thumbnail_url = format!("https://i.ytimg.com/vi/{clean_id}/hqdefault.jpg");

    let (updated_queue, updated, idx, dur) = {
        let mut q_guard = container.lock_queue();
        let mut state = container.lock_player();

        let (target_idx, resolved_title, resolved_artist, resolved_duration) =
            if let Some(existing_pos) = q_guard.iter().position(|t| t.video_id == clean_id) {
                let existing = &mut q_guard[existing_pos];
                if let Some(t) = custom_title {
                    existing.title = t;
                }
                if let Some(a) = custom_artist {
                    existing.artist = a;
                }
                (
                    existing_pos,
                    existing.title.clone(),
                    existing.artist.clone(),
                    existing.duration_secs,
                )
            } else {
                let title_str =
                    custom_title.unwrap_or_else(|| format!("YouTube Stream ({})", clean_id));
                let artist_str =
                    custom_artist.unwrap_or_else(|| "YouTube Music Direct Stream".to_string());
                let new_track = TrackItem {
                    video_id: clean_id.clone(),
                    title: title_str.clone(),
                    artist: artist_str.clone(),
                    album: String::new(),
                    duration_text: String::new(),
                    duration_secs: 0.0,
                    thumbnail_url: thumbnail_url.clone(),
                };
                if q_guard.len() >= MAX_RADIO_QUEUE_LEN {
                    let drain_count = (q_guard.len() + 1).saturating_sub(MAX_RADIO_QUEUE_LEN);
                    if drain_count > 0 && drain_count < q_guard.len() {
                        q_guard.drain(0..drain_count);
                        state.queue_index = state.queue_index.saturating_sub(drain_count);
                    }
                }
                q_guard.push(new_track);
                (
                    q_guard.len() - 1,
                    title_str,
                    artist_str,
                    state.duration_secs,
                )
            };

        state.queue_index = target_idx;
        state.queue_len = q_guard.len();
        state.video_id = clean_id.clone();
        state.current_track_title = resolved_title;
        state.current_track_artist = resolved_artist;
        state.current_thumbnail_url = thumbnail_url;
        state.position_secs = 0.0;
        if resolved_duration > 1.0 {
            state.duration_secs = resolved_duration;
        }
        state.buffer_size_mb = 0.0;
        state.is_loading = true;
        state.is_playing = true;
        state.error_message = None;
        (
            q_guard.clone(),
            state.clone(),
            state.queue_index,
            state.duration_secs,
        )
    };

    broadcast_queue_change(&app, &updated_queue);
    broadcast_state_change(&app, &updated);
    persist_current_session(&app);
    spawn_track_loader(app, clean_id, idx, dur, !has_custom_title);
    get_player_state(container)
}

// ============================================================================
// Phase 4 & 5 Commands: Queue Drawer, Shuffle, Repeat, Radio Autoplay, Low-RAM Images
// ============================================================================

#[tauri::command]
fn get_queue(container: State<'_, AppContainer>) -> Vec<TrackItem> {
    container.lock_queue().clone()
}

#[tauri::command]
fn add_to_queue(
    app: AppHandle,
    container: State<'_, AppContainer>,
    track: TrackItem,
) -> Vec<TrackItem> {
    let trimmed_id = track.video_id.trim();
    if trimmed_id.is_empty() {
        return get_queue(container);
    }

    let (updated_queue, new_len) = {
        let mut q_guard = container.lock_queue();
        if !q_guard.iter().any(|t| t.video_id == trimmed_id) && q_guard.len() < 500 {
            q_guard.push(track);
        }
        (q_guard.clone(), q_guard.len())
    };

    let (updated_state, should_prefetch) = {
        let mut state = container.lock_player();
        state.queue_len = new_len;
        let prefetch =
            !state.is_loading && state.buffer_size_mb > 0.0 && state.queue_index + 1 < new_len;
        (state.clone(), prefetch)
    };

    broadcast_queue_change(&app, &updated_queue);
    broadcast_state_change(&app, &updated_state);
    persist_current_session(&app);
    if should_prefetch {
        spawn_prefetch_next_track(app);
    }
    updated_queue
}

#[tauri::command]
fn play_queue_index(
    app: AppHandle,
    container: State<'_, AppContainer>,
    index: usize,
) -> PlayerState {
    let queue_snapshot = container.lock_queue().clone();

    let (updated, vid, idx, dur) = {
        let mut state = container.lock_player();
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
    persist_current_session(&app);
    spawn_track_loader(app, vid, idx, dur, false);
    get_player_state(container)
}

#[tauri::command]
fn remove_from_queue(
    app: AppHandle,
    container: State<'_, AppContainer>,
    index: usize,
) -> Vec<TrackItem> {
    let mut updated_state: Option<PlayerState> = None;
    let mut should_prefetch = false;
    let mut track_to_load: Option<(String, usize, f64)> = None;
    let mut queue_became_empty = false;

    let updated_queue = {
        let mut q_guard = container.lock_queue();
        if index < q_guard.len() {
            let mut state = container.lock_player();
            let removed_active = index == state.queue_index;
            q_guard.remove(index);

            if removed_active {
                if q_guard.is_empty() {
                    queue_became_empty = true;
                    state.is_playing = false;
                    state.is_loading = false;
                    state.queue_index = 0;
                    state.queue_len = 0;
                    state.video_id.clear();
                    state.current_track_title = "Fila vazia".to_string();
                    state.current_track_artist.clear();
                    state.current_thumbnail_url.clear();
                    state.position_secs = 0.0;
                    state.duration_secs = 0.0;
                    state.buffer_size_mb = 0.0;
                    state.audio_quality_label = "Nenhuma faixa na fila".to_string();
                    state.error_message = None;
                } else {
                    let next_idx = state.queue_index.min(q_guard.len() - 1);
                    state.apply_track_from_queue(&q_guard, next_idx);
                    state.is_loading = true;
                    state.is_playing = true;
                    track_to_load = Some((
                        state.video_id.clone(),
                        state.queue_index,
                        state.duration_secs,
                    ));
                }
            } else {
                if index < state.queue_index {
                    state.queue_index = state.queue_index.saturating_sub(1);
                } else if !q_guard.is_empty() && state.queue_index >= q_guard.len() {
                    state.queue_index = q_guard.len() - 1;
                }
                state.queue_len = q_guard.len();
                should_prefetch = !state.is_loading && state.buffer_size_mb > 0.0;
            }
            updated_state = Some(state.clone());
        }
        q_guard.clone()
    };

    if queue_became_empty {
        container.download_generation.fetch_add(1, Ordering::SeqCst);
        container.prefetch_generation.fetch_add(1, Ordering::SeqCst);
        cancel_and_replace(&container.active_stream_cancel, None);
        cancel_and_replace(&container.prefetch_stream_cancel, None);
        {
            let mut guard = container
                .prefetched_audio
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *guard = None;
        }
        {
            let mut buf_guard = container
                .active_buffer
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *buf_guard = None;
        }
        container.audio_engine.send(AudioCommand::Stop);
    }

    broadcast_queue_change(&app, &updated_queue);
    if let Some(st) = updated_state {
        broadcast_state_change(&app, &st);
    }
    persist_current_session(&app);

    if let Some((vid, idx, dur)) = track_to_load {
        spawn_track_loader(app, vid, idx, dur, false);
    } else if should_prefetch {
        spawn_prefetch_next_track(app);
    }
    updated_queue
}

#[tauri::command]
fn clear_queue(app: AppHandle, container: State<'_, AppContainer>) -> Vec<TrackItem> {
    // Cancel any in-flight prefetch and release stale prefetched RAM immediately
    container.prefetch_generation.fetch_add(1, Ordering::SeqCst);
    cancel_and_replace(&container.prefetch_stream_cancel, None);
    {
        let mut guard = container
            .prefetched_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    let (updated_queue, updated_state, should_prefetch) = {
        let mut q_guard = container.lock_queue();
        let mut state = container.lock_player();
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
        let prefetch = !state.is_loading && state.buffer_size_mb > 0.0;
        (q_guard.clone(), state.clone(), prefetch)
    };

    broadcast_queue_change(&app, &updated_queue);
    broadcast_state_change(&app, &updated_state);
    persist_current_session(&app);
    if should_prefetch {
        spawn_prefetch_next_track(app);
    }
    updated_queue
}

#[tauri::command]
fn shuffle_queue(app: AppHandle, container: State<'_, AppContainer>) -> Vec<TrackItem> {
    let mut updated_state: Option<PlayerState> = None;
    let mut should_prefetch = false;

    let updated_queue = {
        let mut q_guard = container.lock_queue();
        if q_guard.len() > 2 {
            let current_idx = {
                let state = container.lock_player();
                state.queue_index.min(q_guard.len() - 1)
            };

            // Keep currently playing track at index 0 so playback is not interrupted
            q_guard.swap(0, current_idx);

            // Lightweight xorshift64* PRNG seeded from system nanoseconds for Fisher-Yates shuffle
            let mut seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                ^ 0xBF58_476D_1CE4_E5B9;
            if seed == 0 {
                seed = 1;
            }

            let len = q_guard.len();
            for i in (1..len).rev() {
                seed ^= seed >> 12;
                seed ^= seed << 25;
                seed ^= seed >> 27;
                let rand_val = seed.wrapping_mul(0x2545_F491_4F6C_DD1D);
                let j = 1 + (rand_val as usize % i);
                q_guard.swap(i, j);
            }

            let mut state = container.lock_player();
            state.queue_index = 0;
            state.queue_len = q_guard.len();
            should_prefetch = !state.is_loading && state.buffer_size_mb > 0.0;
            updated_state = Some(state.clone());
        }
        q_guard.clone()
    };

    broadcast_queue_change(&app, &updated_queue);
    if let Some(st) = updated_state {
        broadcast_state_change(&app, &st);
    }
    persist_current_session(&app);
    if should_prefetch {
        spawn_prefetch_next_track(app);
    }
    updated_queue
}

#[tauri::command]
fn cycle_repeat_mode(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let (updated, should_prefetch) = {
        let mut state = container.lock_player();
        state.repeat_mode = state.repeat_mode.cycle();
        let prefetch = !state.is_loading && state.buffer_size_mb > 0.0;
        (state.clone(), prefetch)
    };
    broadcast_state_change(&app, &updated);
    persist_current_session(&app);
    if should_prefetch {
        spawn_prefetch_next_track(app);
    }
    updated
}

#[tauri::command]
fn toggle_autoplay_radio(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let (updated, should_prefetch) = {
        let mut state = container.lock_player();
        state.autoplay_radio = !state.autoplay_radio;
        let prefetch = !state.is_loading && state.buffer_size_mb > 0.0;
        (state.clone(), prefetch)
    };
    broadcast_state_change(&app, &updated);
    persist_current_session(&app);
    if should_prefetch {
        spawn_prefetch_next_track(app);
    }
    updated
}

#[tauri::command]
fn toggle_low_ram_images(app: AppHandle, container: State<'_, AppContainer>) -> PlayerState {
    let updated = {
        let mut state = container.lock_player();
        state.low_ram_no_images = !state.low_ram_no_images;
        state.clone()
    };
    broadcast_state_change(&app, &updated);
    persist_current_session(&app);
    updated
}

// ============================================================================
// Phase 3 Commands: Google Auth (`SAPISIDHASH`), Innertube API & Lyrics
// ============================================================================

#[tauri::command]
fn get_auth_status(container: State<'_, AppContainer>) -> AuthStatus {
    container
        .auth
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .to_status()
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

    {
        let mut guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
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

    {
        let mut guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
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
    let auth_opt = {
        let guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_authenticated {
            Some(guard.clone())
        } else {
            None
        }
    };
    fetch_home(auth_opt.as_ref()).await
}

#[tauri::command]
async fn search_youtube_music(
    container: State<'_, AppContainer>,
    query: String,
    filter: Option<String>,
) -> Result<Vec<TrackItem>, String> {
    let auth_opt = {
        let guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_authenticated {
            Some(guard.clone())
        } else {
            None
        }
    };
    search_music_filtered(&query, filter.as_deref(), auth_opt.as_ref()).await
}

#[tauri::command]
async fn search_ytm(
    container: State<'_, AppContainer>,
    query: String,
    filter: Option<String>,
) -> Result<Vec<TrackItem>, String> {
    search_youtube_music(container, query, filter).await
}

#[tauri::command]
async fn get_track_lyrics(
    container: State<'_, AppContainer>,
    video_id: String,
    title: Option<String>,
    artist: Option<String>,
) -> Result<LyricsResponse, String> {
    let auth_opt = {
        let guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_authenticated {
            Some(guard.clone())
        } else {
            None
        }
    };
    fetch_track_lyrics(
        &video_id,
        title.as_deref(),
        artist.as_deref(),
        auth_opt.as_ref(),
    )
    .await
}

#[tauri::command]
fn get_auth_file_path_str() -> String {
    auth::get_auth_file_path().to_string_lossy().to_string()
}

#[tauri::command]
fn toggle_like_track(
    app: AppHandle,
    container: State<'_, AppContainer>,
    track: TrackItem,
) -> Vec<TrackItem> {
    let vid = track.video_id.trim().to_string();
    if vid.is_empty() {
        return container
            .liked_tracks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
    }

    let (updated_liked, is_now_liked) = {
        let mut guard = container
            .liked_tracks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = guard.iter().position(|t| t.video_id == vid) {
            guard.remove(pos);
            (guard.clone(), false)
        } else {
            guard.insert(0, track);
            if guard.len() > 500 {
                guard.truncate(500);
            }
            (guard.clone(), true)
        }
    };

    persist_current_session(&app);

    if let Some(auth) = current_auth_snapshot(&app) {
        tauri::async_runtime::spawn(async move {
            let _ = set_track_like_status(&vid, is_now_liked, &auth).await;
        });
    }

    updated_liked
}

#[tauri::command]
async fn get_liked_music(
    app: AppHandle,
    container: State<'_, AppContainer>,
) -> Result<Vec<TrackItem>, String> {
    let auth_opt = {
        let guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_authenticated && !guard.sapisid.is_empty() {
            Some(guard.clone())
        } else {
            None
        }
    };

    if let Some(auth) = auth_opt {
        let tracks = fetch_liked_music(Some(&auth)).await?;
        if !tracks.is_empty() {
            {
                let mut guard = container
                    .liked_tracks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                *guard = tracks.clone();
            }
            persist_current_session(&app);
        }
        return Ok(tracks);
    }

    // Guest mode: preserve user's locally liked tracks across tab switches & restarts
    let local_liked = container
        .liked_tracks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Ok(local_liked)
}

#[tauri::command]
async fn get_user_playlists(
    container: State<'_, AppContainer>,
) -> Result<Vec<PlaylistItem>, String> {
    let auth_opt = {
        let guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_authenticated {
            Some(guard.clone())
        } else {
            None
        }
    };
    fetch_user_playlists(auth_opt.as_ref()).await
}

#[tauri::command]
async fn get_playlist_tracks(
    container: State<'_, AppContainer>,
    playlist_id: String,
) -> Result<Vec<TrackItem>, String> {
    let auth_opt = {
        let guard = container.auth.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_authenticated {
            Some(guard.clone())
        } else {
            None
        }
    };
    fetch_playlist_tracks(&playlist_id, auth_opt.as_ref()).await
}

fn setup_system_tray(app: &AppHandle) -> tauri::Result<()> {
    let initial_status_text = {
        let container = app.state::<AppContainer>();
        let state = container.lock_player();
        let text = state.format_tray_status();
        {
            let mut last_text = container
                .last_tray_text
                .lock()
                .unwrap_or_else(|e| e.into_inner());
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
            .unwrap_or_else(|e| e.into_inner());
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
                if let Some(session) = snapshot_persisted_session(app) {
                    save_persisted_session(&session);
                }
                audio::clean_temp_cookie_files();
                let container = app.state::<AppContainer>();
                container.download_generation.fetch_add(1, Ordering::SeqCst);
                container.prefetch_generation.fetch_add(1, Ordering::SeqCst);
                cancel_and_replace(&container.active_stream_cancel, None);
                cancel_and_replace(&container.prefetch_stream_cancel, None);
                {
                    let mut guard = container
                        .prefetched_audio
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    *guard = None;
                }
                {
                    let mut buf_guard = container
                        .active_buffer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    *buf_guard = None;
                }
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
            audio::clean_temp_cookie_files();
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
            play_track_list,
            set_volume,
            seek_to,
            clear_error,
            play_youtube_video,
            get_queue,
            add_to_queue,
            play_queue_index,
            remove_from_queue,
            clear_queue,
            shuffle_queue,
            cycle_repeat_mode,
            toggle_autoplay_radio,
            toggle_low_ram_images,
            get_auth_status,
            get_auth_file_path_str,
            open_login_window,
            set_manual_cookie,
            logout_auth,
            get_home_feed,
            search_youtube_music,
            search_ytm,
            get_track_lyrics,
            toggle_like_track,
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
    app.run(|app_handle, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            let quit_requested = app_handle
                .try_state::<AppContainer>()
                .map(|c| c.quit_requested.load(Ordering::SeqCst))
                .unwrap_or(false);

            if !quit_requested {
                api.prevent_exit();
                #[cfg(target_os = "macos")]
                let _ = app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            if !has_visible_windows {
                show_or_recreate_main_window(app_handle);
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_track(id: &str) -> TrackItem {
        TrackItem {
            video_id: id.to_string(),
            title: format!("Track {id}"),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_text: "3:00".to_string(),
            duration_secs: 180.0,
            thumbnail_url: format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"),
        }
    }

    #[test]
    fn test_determine_next_video_id_modes() {
        let queue = default_queue_tracks();
        assert_eq!(queue.len(), 4);

        // Empty queue -> None
        assert_eq!(determine_next_video_id(&[], 0, RepeatMode::Off), None);

        // RepeatMode::One -> None (already in RAM buffer)
        assert_eq!(determine_next_video_id(&queue, 0, RepeatMode::One), None);

        // Middle of queue -> next track (index + 1)
        assert_eq!(
            determine_next_video_id(&queue, 0, RepeatMode::Off),
            Some(queue[1].video_id.clone())
        );
        assert_eq!(
            determine_next_video_id(&queue, 2, RepeatMode::All),
            Some(queue[3].video_id.clone())
        );

        // End of queue with RepeatMode::All -> wraps to index 0
        assert_eq!(
            determine_next_video_id(&queue, 3, RepeatMode::All),
            Some(queue[0].video_id.clone())
        );

        // End of queue with RepeatMode::Off -> None (handled by Radio Autoplay prefetch if enabled)
        assert_eq!(determine_next_video_id(&queue, 3, RepeatMode::Off), None);
    }

    #[test]
    fn test_apply_track_resets_buffer_size() {
        let queue = default_queue_tracks();
        let mut state = PlayerState::default();
        state.buffer_size_mb = 3.5;
        state.apply_track_from_queue(&queue, 1);
        assert_eq!(state.buffer_size_mb, 0.0);
        assert_eq!(state.queue_index, 1);
        assert_eq!(state.video_id, queue[1].video_id);
    }

    #[test]
    fn test_append_radio_tracks_capped_sliding_window_and_dedup() {
        let mut queue: Vec<TrackItem> = (0..300).map(|i| make_track(&format!("old_{i}"))).collect();
        let mut state = PlayerState::default();
        state.apply_track_from_queue(&queue, 299);
        let active_vid = queue[299].video_id.clone();

        let mut incoming: Vec<TrackItem> =
            (0..20).map(|i| make_track(&format!("new_{i}"))).collect();
        // Include a duplicate of an existing track
        incoming.push(make_track("old_299"));

        let added = append_radio_tracks_capped(&mut queue, &mut state, incoming, 300);
        assert_eq!(added, 20);
        assert_eq!(queue.len(), 300);
        assert_eq!(state.queue_len, 300);
        assert_eq!(state.queue_index, 279);
        assert_eq!(queue[state.queue_index].video_id, active_vid);
        assert_eq!(queue[state.queue_index + 1].video_id, "new_0");
        assert_eq!(queue[299].video_id, "new_19");
    }

    #[test]
    fn test_cancel_and_replace_cancels_previous_but_not_same_arc() {
        let slot: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
        let first = Arc::new(AtomicBool::new(false));
        let second = Arc::new(AtomicBool::new(false));

        cancel_and_replace(&slot, Some(first.clone()));
        assert!(!first.load(Ordering::SeqCst));

        // Replacing with the same Arc does not self-cancel
        cancel_and_replace(&slot, Some(first.clone()));
        assert!(!first.load(Ordering::SeqCst));

        // Replacing with a different Arc cancels the first
        cancel_and_replace(&slot, Some(second.clone()));
        assert!(first.load(Ordering::SeqCst));
        assert!(!second.load(Ordering::SeqCst));

        // Clearing cancels the active flag
        cancel_and_replace(&slot, None);
        assert!(second.load(Ordering::SeqCst));
    }
}
