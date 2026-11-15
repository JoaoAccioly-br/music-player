use crate::auth::{generate_sapisidhash, AuthSession, FIREFOX_LINUX_UA, YTM_ORIGIN};
use rodio::{Decoder, OutputStream, Sink, Source};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub const YT_DLP_LINUX_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";

static COOKIE_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
static YT_DLP_INIT_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Parses the ISO-BMFF / MP4 `sidx` (Segment Index) box located in the initial header (~2 KB)
/// of YouTube DASH M4A (`itag 140`) streams to determine the exact total byte length and
/// exact duration in seconds before the rest of the stream has finished downloading.
pub fn parse_mp4_sidx_metadata(data: &[u8]) -> Option<(u64, f64)> {
    let mut pos: usize = 0;
    while pos + 8 <= data.len() {
        let size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let box_type = &data[pos + 4..pos + 8];
        if size < 8 {
            break;
        }
        if box_type == b"sidx" {
            if pos + size > data.len() || size < 32 {
                return None;
            }
            let sidx_end = (pos + size) as u64;
            let version = data[pos + 8];
            let timescale = u32::from_be_bytes([
                data[pos + 16],
                data[pos + 17],
                data[pos + 18],
                data[pos + 19],
            ]);
            let (first_offset, ref_pos) = if version == 0 {
                let fo = u32::from_be_bytes([
                    data[pos + 24],
                    data[pos + 25],
                    data[pos + 26],
                    data[pos + 27],
                ]) as u64;
                (fo, pos + 28)
            } else {
                if pos + 40 > data.len() {
                    return None;
                }
                let fo = u64::from_be_bytes([
                    data[pos + 28],
                    data[pos + 29],
                    data[pos + 30],
                    data[pos + 31],
                    data[pos + 32],
                    data[pos + 33],
                    data[pos + 34],
                    data[pos + 35],
                ]);
                (fo, pos + 36)
            };

            if ref_pos + 4 > pos + size {
                return None;
            }
            let ref_count = u16::from_be_bytes([data[ref_pos + 2], data[ref_pos + 3]]) as usize;
            let mut cur = ref_pos + 4;
            if cur + ref_count * 12 > pos + size {
                return None;
            }

            let mut total_ref_size: u64 = 0;
            let mut total_dur_ticks: u64 = 0;
            for _ in 0..ref_count {
                let raw_size =
                    u32::from_be_bytes([data[cur], data[cur + 1], data[cur + 2], data[cur + 3]]);
                let sub_dur = u32::from_be_bytes([
                    data[cur + 4],
                    data[cur + 5],
                    data[cur + 6],
                    data[cur + 7],
                ]);
                total_ref_size += (raw_size & 0x7FFF_FFFF) as u64;
                total_dur_ticks += sub_dur as u64;
                cur += 12;
            }

            let exact_total_bytes = sidx_end + first_offset + total_ref_size;
            let duration_secs = if timescale > 0 {
                (total_dur_ticks as f64) / (timescale as f64)
            } else {
                0.0
            };
            return Some((exact_total_bytes, duration_secs));
        }

        if pos + size > data.len() {
            break;
        }
        pos += size;
    }
    None
}

struct ProgressiveState {
    data: Vec<u8>,
    finished: bool,
    total_expected: Option<u64>,
    error: Option<String>,
}

struct ProgressiveShared {
    state: Mutex<ProgressiveState>,
    cvar: Condvar,
}

/// Incremental, zero-copy (shared via `Arc`) audio buffer that supports progressive
/// streaming in chunks while `rodio::Decoder` decodes and plays concurrently.
#[derive(Clone)]
pub struct ProgressiveAudioBuffer {
    shared: Arc<ProgressiveShared>,
}

impl std::fmt::Debug for ProgressiveAudioBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressiveAudioBuffer")
            .field("len", &self.len())
            .field("finished", &self.is_finished())
            .finish()
    }
}

impl PartialEq for ProgressiveAudioBuffer {
    fn eq(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.shared, &other.shared) {
            return true;
        }
        let Ok(g1) = self.shared.state.lock() else {
            return false;
        };
        let Ok(g2) = other.shared.state.lock() else {
            return false;
        };
        g1.data == g2.data
    }
}

impl Eq for ProgressiveAudioBuffer {}

impl From<Vec<u8>> for ProgressiveAudioBuffer {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_vec(bytes)
    }
}

impl ProgressiveAudioBuffer {
    /// Creates a new in-flight progressive buffer with pre-allocated capacity.
    pub fn new(capacity: usize, total_expected: Option<u64>) -> Self {
        Self {
            shared: Arc::new(ProgressiveShared {
                state: Mutex::new(ProgressiveState {
                    data: Vec::with_capacity(capacity),
                    finished: false,
                    total_expected,
                    error: None,
                }),
                cvar: Condvar::new(),
            }),
        }
    }

    /// Creates a completed `ProgressiveAudioBuffer` from an existing `Vec<u8>`.
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let len = bytes.len() as u64;
        Self {
            shared: Arc::new(ProgressiveShared {
                state: Mutex::new(ProgressiveState {
                    data: bytes,
                    finished: true,
                    total_expected: Some(len),
                    error: None,
                }),
                cvar: Condvar::new(),
            }),
        }
    }

    /// Appends a chunk of bytes and wakes up any waiting `ProgressiveReader` threads.
    pub fn push_chunk(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.shared.state.lock() {
            guard.data.extend_from_slice(chunk);
            if guard.total_expected.is_none() {
                if let Some((exact_bytes, _)) = parse_mp4_sidx_metadata(&guard.data) {
                    guard.total_expected = Some(exact_bytes.min(25 * 1024 * 1024));
                }
            }
            self.shared.cvar.notify_all();
        }
    }

    /// Sets the expected total byte length of the progressive stream.
    pub fn set_total_expected(&self, expected: u64) {
        if let Ok(mut guard) = self.shared.state.lock() {
            guard.total_expected = Some(expected);
            self.shared.cvar.notify_all();
        }
    }

    /// Inspects buffered header bytes to extract DASH MP4 `sidx` `(exact_total_bytes, duration_secs)`.
    pub fn inspect_mp4_sidx(&self) -> Option<(u64, f64)> {
        let guard = self.shared.state.lock().ok()?;
        parse_mp4_sidx_metadata(&guard.data)
    }

    /// Marks the progressive stream as completely downloaded and wakes all readers.
    pub fn mark_finished(&self) {
        if let Ok(mut guard) = self.shared.state.lock() {
            guard.finished = true;
            let final_len = guard.data.len() as u64;
            guard.total_expected = Some(final_len);
            self.shared.cvar.notify_all();
        }
    }

    /// Records a stream error, marks the buffer as finished, and wakes all readers.
    pub fn mark_error(&self, err: String) {
        if let Ok(mut guard) = self.shared.state.lock() {
            guard.error = Some(err);
            guard.finished = true;
            self.shared.cvar.notify_all();
        }
    }

    /// Returns the number of bytes currently buffered in RAM.
    pub fn len(&self) -> usize {
        self.shared.state.lock().map(|g| g.data.len()).unwrap_or(0)
    }

    /// Alias for `len()` returning current buffered byte length.
    pub fn current_bytes_len(&self) -> usize {
        self.len()
    }

    /// Returns `true` if no bytes have been buffered yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` once the background stream has finished (or errored/EOF).
    pub fn is_finished(&self) -> bool {
        self.shared.state.lock().map(|g| g.finished).unwrap_or(true)
    }

    /// Creates a new `Read + Seek` reader starting at byte offset 0.
    pub fn reader(&self) -> ProgressiveReader {
        ProgressiveReader {
            buffer: self.clone(),
            pos: 0,
        }
    }
}

/// A `Read + Seek` cursor over `ProgressiveAudioBuffer` that blocks on a `Condvar`
/// when reading or seeking into regions that are still arriving from the network.
pub struct ProgressiveReader {
    buffer: ProgressiveAudioBuffer,
    pos: u64,
}

impl Read for ProgressiveReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut state = self
            .buffer
            .shared
            .state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Progressive mutex poisoned"))?;

        while (self.pos as usize) >= state.data.len() && !state.finished {
            let (new_state, timeout_res) = self
                .buffer
                .shared
                .cvar
                .wait_timeout(state, Duration::from_secs(20))
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "Condvar wait poisoned"))?;
            state = new_state;
            if timeout_res.timed_out() && (self.pos as usize) >= state.data.len() && !state.finished
            {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Timeout aguardando chunks de áudio da rede",
                ));
            }
        }

        let cur_pos = self.pos as usize;
        if cur_pos < state.data.len() {
            let avail = state.data.len() - cur_pos;
            let n = avail.min(buf.len());
            buf[..n].copy_from_slice(&state.data[cur_pos..cur_pos + n]);
            self.pos += n as u64;
            return Ok(n);
        }

        if let Some(ref err_msg) = state.error {
            if state.data.is_empty() {
                return Err(io::Error::new(io::ErrorKind::Other, err_msg.clone()));
            }
        }

        Ok(0)
    }
}

impl Seek for ProgressiveReader {
    fn seek(&mut self, style: SeekFrom) -> io::Result<u64> {
        let mut state = self
            .buffer
            .shared
            .state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Progressive mutex poisoned"))?;

        let target: i128 = match style {
            SeekFrom::Start(offset) => offset as i128,
            SeekFrom::Current(delta) => (self.pos as i128) + (delta as i128),
            SeekFrom::End(delta) => {
                // `ReadSeekSource::new()` calls `seek(SeekFrom::End(0))` followed by
                // `seek(SeekFrom::Start(current_pos))` to query `byte_len()`.
                // For YouTube DASH MP4/M4A (`itag 140`), `parse_mp4_sidx_metadata` computes the
                // exact total byte size from the `sidx` box in the first 2 KB of the stream.
                if !state.finished && state.total_expected.is_none() {
                    if let Some((exact_bytes, _)) = parse_mp4_sidx_metadata(&state.data) {
                        state.total_expected = Some(exact_bytes.min(25 * 1024 * 1024));
                    } else {
                        while !state.finished && state.total_expected.is_none() {
                            let (new_state, timeout_res) = self
                                .buffer
                                .shared
                                .cvar
                                .wait_timeout(state, Duration::from_secs(20))
                                .map_err(|_| {
                                    io::Error::new(io::ErrorKind::Other, "Condvar wait poisoned")
                                })?;
                            state = new_state;
                            if timeout_res.timed_out() {
                                break;
                            }
                        }
                    }
                }

                let end_len = if state.finished {
                    state.data.len() as u64
                } else {
                    state.total_expected.unwrap_or(state.data.len() as u64)
                };
                (end_len as i128) + (delta as i128)
            }
        };

        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Seek antes do início do stream",
            ));
        }

        let target_u64 = target as u64;

        // If a decoder/demuxer attempts to seek to a specific byte offset beyond the currently
        // downloaded data, wait on the Condvar until `data.len() >= target` or the stream finishes.
        if !matches!(style, SeekFrom::End(_)) {
            while (target_u64 as usize) > state.data.len() && !state.finished {
                let (new_state, timeout_res) = self
                    .buffer
                    .shared
                    .cvar
                    .wait_timeout(state, Duration::from_secs(20))
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "Condvar wait poisoned"))?;
                state = new_state;
                if timeout_res.timed_out()
                    && (target_u64 as usize) > state.data.len()
                    && !state.finished
                {
                    break;
                }
            }
        }

        self.pos = if state.finished && !matches!(style, SeekFrom::End(_)) {
            target_u64.min(state.data.len() as u64)
        } else {
            target_u64
        };

        Ok(self.pos)
    }
}

/// Real-time lock-free atomic telemetry updated by the dedicated native audio thread (`rodio::Sink`).
pub struct AudioTelemetry {
    pub position_ms: AtomicU64,
    pub is_sink_empty: AtomicBool,
    pub has_active_track: AtomicBool,
}

impl Default for AudioTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioTelemetry {
    pub fn new() -> Self {
        Self {
            position_ms: AtomicU64::new(0),
            is_sink_empty: AtomicBool::new(true),
            has_active_track: AtomicBool::new(false),
        }
    }

    pub fn position_seconds(&self) -> f64 {
        (self.position_ms.load(Ordering::Relaxed) as f64) / 1000.0
    }
}

/// Commands sent from Tauri IPC / System Tray threads to the dedicated native audio thread.
/// Because `rodio::OutputStream` contains raw OS audio device handles (ALSA/PipeWire) that are
/// `!Send` and `!Sync`, keeping `OutputStream` and `Sink` inside a dedicated OS thread and
/// communicating via an `mpsc::Sender<AudioCommand>` is the idiomatic, zero-overhead Rust pattern.
pub enum AudioCommand {
    LoadAndPlay {
        data: ProgressiveAudioBuffer,
        volume: f32,
    },
    SetPlaying(bool),
    SetVolume(f32),
    Seek(f64),
    Stop,
}

/// Thread-safe handle to the dedicated native audio thread.
pub struct AudioEngine {
    sender: Mutex<Sender<AudioCommand>>,
    pub telemetry: Arc<AudioTelemetry>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        let (tx, rx) = channel::<AudioCommand>();
        let telemetry = Arc::new(AudioTelemetry::new());
        let thread_telemetry = telemetry.clone();

        thread::Builder::new()
            .name("ytm-native-audio".to_string())
            .spawn(move || {
                // Attempt to open the default OS audio output device (ALSA / PulseAudio / PipeWire).
                // `_stream` MUST stay alive on this thread for `Sink` to output sound.
                let mut stream_opt = OutputStream::try_default().ok();
                let mut sink_opt: Option<Sink> = None;
                let mut current_buffer: Option<ProgressiveAudioBuffer> = None;
                let mut current_volume: f32 = 0.8;
                let mut is_paused: bool = false;
                let mut seek_offset_secs: f64 = 0.0;

                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(cmd) => {
                            // Lazily retry opening the audio device if it wasn't available at startup.
                            if stream_opt.is_none() {
                                stream_opt = OutputStream::try_default().ok();
                            }

                            match cmd {
                                AudioCommand::LoadAndPlay { data, volume } => {
                                    current_volume = volume.clamp(0.0, 1.0);
                                    is_paused = false;
                                    seek_offset_secs = 0.0;

                                    // Drop previous sink immediately to stop playback and free decoder memory.
                                    if let Some(old_sink) = sink_opt.take() {
                                        old_sink.stop();
                                    }

                                    current_buffer = Some(data.clone());
                                    thread_telemetry.position_ms.store(0, Ordering::Relaxed);
                                    thread_telemetry
                                        .is_sink_empty
                                        .store(false, Ordering::Relaxed);
                                    thread_telemetry
                                        .has_active_track
                                        .store(true, Ordering::Relaxed);

                                    if let Some((_, ref stream_handle)) = stream_opt {
                                        if let Ok(new_sink) = Sink::try_new(stream_handle) {
                                            new_sink.set_volume(current_volume);
                                            if let Ok(decoder) = Decoder::new(data.reader()) {
                                                new_sink.append(decoder);
                                                new_sink.play();
                                                sink_opt = Some(new_sink);
                                            }
                                        }
                                    }
                                }
                                AudioCommand::SetPlaying(should_play) => {
                                    is_paused = !should_play;
                                    if let Some(ref sink) = sink_opt {
                                        if should_play {
                                            sink.play();
                                        } else {
                                            sink.pause();
                                        }
                                    }
                                }
                                AudioCommand::SetVolume(vol) => {
                                    current_volume = vol.clamp(0.0, 1.0);
                                    if let Some(ref sink) = sink_opt {
                                        sink.set_volume(current_volume);
                                    }
                                }
                                AudioCommand::Seek(position_secs) => {
                                    let clamped_secs = position_secs.max(0.0);
                                    let target = Duration::from_secs_f64(clamped_secs);
                                    let mut seek_succeeded = false;

                                    if let Some(ref sink) = sink_opt {
                                        if !sink.empty() && sink.try_seek(target).is_ok() {
                                            seek_succeeded = true;
                                            seek_offset_secs = 0.0;
                                        }
                                    }

                                    // Fallback if the sink had already finished or decoder required re-seeking from RAM buffer
                                    if !seek_succeeded {
                                        if let (Some((_, ref stream_handle)), Some(ref buffer)) =
                                            (&stream_opt, &current_buffer)
                                        {
                                            if let Some(old_sink) = sink_opt.take() {
                                                old_sink.stop();
                                            }
                                            if let Ok(new_sink) = Sink::try_new(stream_handle) {
                                                new_sink.set_volume(current_volume);
                                                if let Ok(mut decoder) =
                                                    Decoder::new(buffer.reader())
                                                {
                                                    if decoder.try_seek(target).is_ok() {
                                                        seek_offset_secs = clamped_secs;
                                                        new_sink.append(decoder);
                                                    } else if let Ok(dec_skip) =
                                                        Decoder::new(buffer.reader())
                                                    {
                                                        seek_offset_secs = clamped_secs;
                                                        new_sink
                                                            .append(dec_skip.skip_duration(target));
                                                    }
                                                    if is_paused {
                                                        new_sink.pause();
                                                    } else {
                                                        new_sink.play();
                                                    }
                                                    sink_opt = Some(new_sink);
                                                }
                                            }
                                        } else {
                                            seek_offset_secs = clamped_secs;
                                        }
                                    }

                                    thread_telemetry
                                        .position_ms
                                        .store((clamped_secs * 1000.0) as u64, Ordering::Relaxed);
                                }
                                AudioCommand::Stop => {
                                    if let Some(old_sink) = sink_opt.take() {
                                        old_sink.stop();
                                    }
                                    current_buffer = None;
                                    seek_offset_secs = 0.0;
                                    thread_telemetry.position_ms.store(0, Ordering::Relaxed);
                                    thread_telemetry
                                        .is_sink_empty
                                        .store(true, Ordering::Relaxed);
                                    thread_telemetry
                                        .has_active_track
                                        .store(false, Ordering::Relaxed);
                                }
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }

                    // Update real-time atomic telemetry from active `rodio::Sink`
                    if let Some(ref sink) = sink_opt {
                        let pos_secs = seek_offset_secs + sink.get_pos().as_secs_f64();
                        thread_telemetry
                            .position_ms
                            .store((pos_secs * 1000.0) as u64, Ordering::Relaxed);
                        let is_empty = sink.empty();
                        let buffer_done = current_buffer
                            .as_ref()
                            .map(|b| b.is_finished())
                            .unwrap_or(true);
                        thread_telemetry
                            .is_sink_empty
                            .store(is_empty && buffer_done && !is_paused, Ordering::Relaxed);
                    }
                }
            })
            .expect("failed to spawn native audio thread");

        Self {
            sender: Mutex::new(tx),
            telemetry,
        }
    }

    pub fn send(&self, cmd: AudioCommand) {
        if let Ok(guard) = self.sender.lock() {
            let _ = guard.send(cmd);
        }
    }

    /// Returns the real hardware playback position in seconds from `AudioTelemetry`.
    pub fn get_position_secs(&self) -> f64 {
        self.telemetry.position_seconds()
    }

    /// Returns `true` when an active track is loaded, the progressive buffer has finished
    /// downloading, playback is not paused, and the `rodio::Sink` has drained all samples.
    pub fn is_track_finished(&self) -> bool {
        self.telemetry.has_active_track.load(Ordering::Relaxed)
            && self.telemetry.is_sink_empty.load(Ordering::Relaxed)
    }
}

/// Result of extracting a real YouTube audio stream into RAM (progressive or complete).
#[derive(Debug, Clone)]
pub struct ExtractedAudio {
    pub data: ProgressiveAudioBuffer,
    pub duration_secs: f64,
    pub quality_label: String,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Resolves `~/.local/share/com.ytmusic.rust/bin/yt-dlp`.
pub fn get_yt_dlp_bin_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("com.ytmusic.rust")
        .join("bin")
        .join("yt-dlp")
}

/// Ensures the standalone `yt-dlp` binary exists at `~/.local/share/com.ytmusic.rust/bin/yt-dlp`.
/// If not present on disk, automatically downloads `yt-dlp_linux` from the official GitHub release
/// and sets executable permissions (`0755`). Protected by an async mutex so concurrent calls
/// never race on `yt-dlp.part`.
pub async fn ensure_yt_dlp_binary() -> Result<PathBuf, String> {
    let init_mutex = YT_DLP_INIT_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = init_mutex.lock().await;

    let bin_path = get_yt_dlp_bin_path();
    if bin_path.exists() {
        if let Ok(meta) = fs::metadata(&bin_path) {
            if meta.len() > 1_000_000 {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755));
                }
                return Ok(bin_path);
            }
        }
    }

    if let Some(parent) = bin_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Falha ao criar diretório para yt-dlp ({:?}): {e}", parent))?;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| format!("Falha ao iniciar cliente HTTP para baixar yt-dlp: {e}"))?;

    let resp = client
        .get(YT_DLP_LINUX_URL)
        .header("User-Agent", FIREFOX_LINUX_UA)
        .send()
        .await
        .map_err(|e| format!("Falha ao baixar yt-dlp oficial do GitHub: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Download do yt-dlp retornou HTTP {}",
            resp.status()
        ));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Erro ao ler bytes do binário yt-dlp: {e}"))?;

    let tmp_path = bin_path.with_extension("part");
    fs::write(&tmp_path, &bytes)
        .map_err(|e| format!("Falha ao salvar yt-dlp temporário ({:?}): {e}", tmp_path))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Falha ao aplicar chmod 0755 no yt-dlp: {e}"))?;
    }

    fs::rename(&tmp_path, &bin_path)
        .map_err(|e| format!("Falha ao mover yt-dlp para {:?}: {e}", bin_path))?;

    Ok(bin_path)
}

/// Parses a YouTube or YouTube Music URL (or raw video ID) into a clean 11-character video ID.
pub fn parse_youtube_video_id(input: &str) -> String {
    let trimmed = input.trim();

    // Check query parameter ?v= or &v=
    if let Some(pos) = trimmed.find("v=") {
        let after_v = &trimmed[pos + 2..];
        let id: String = after_v
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if id.len() == 11 {
            return id;
        }
    }

    // Check short URL youtu.be/<id> or /embed/<id> or /shorts/<id>
    for marker in ["youtu.be/", "/embed/", "/shorts/", "/v/"] {
        if let Some(pos) = trimmed.find(marker) {
            let after = &trimmed[pos + marker.len()..];
            let id: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if id.len() == 11 {
                return id;
            }
        }
    }

    // Otherwise return cleaned alphanumeric/-/_ token
    let cleaned: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(11)
        .collect();

    if cleaned.is_empty() {
        "dQw4w9WgXcQ".to_string()
    } else {
        cleaned
    }
}

/// Writes an authenticated Netscape `cookies.txt` file with strict `0600` permissions if needed by `yt-dlp`.
/// Uses a unique filename per invocation so concurrent active downloads and background prefetches never race.
fn write_temp_netscape_cookies(session: &AuthSession) -> Option<PathBuf> {
    if !session.is_authenticated || session.cookie_header.trim().is_empty() {
        return None;
    }

    let seq = COOKIE_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let cookies_path = PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("com.ytmusic.rust")
        .join(format!("cookies.{pid}.{nanos}.{seq}.tmp.txt"));

    if let Some(parent) = cookies_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut lines = vec![
        "# Netscape HTTP Cookie File".to_string(),
        "# Generated in-app by youtube-music-rust".to_string(),
    ];

    for pair in session.cookie_header.split(';') {
        let trimmed = pair.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            let name = k.trim();
            let val = v.trim();
            if !name.is_empty() && !val.is_empty() {
                lines.push(format!(
                    ".youtube.com\tTRUE\t/\tTRUE\t2147483647\t{name}\t{val}"
                ));
            }
        }
    }

    if fs::write(&cookies_path, lines.join("\n")).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&cookies_path, fs::Permissions::from_mode(0o600));
        }
        Some(cookies_path)
    } else {
        None
    }
}

/// Primary extraction engine:
/// 1. Uses the isolated `~/.local/share/com.ytmusic.rust/bin/yt-dlp` binary to stream real YouTube
///    `m4a` (`itag 140` / `bestaudio[ext=m4a]`) bytes directly to `stdout` into a progressive RAM
///    `ProgressiveAudioBuffer`. As soon as the initial 256 KB header (`ftyp` + `moov` + `sidx` + first
///    `moof`/`mdat` audio frames) is buffered and verified by `rodio::Decoder`, returns immediately
///    while a background thread continues feeding the remaining chunks!
///    Supports immediate cancellation via `generation_pair` (`kill_on_drop(true)`) so switching
///    songs quickly kills the old `yt-dlp` process right away.
/// 2. Falls back to direct Innertube `/youtubei/v1/player` extraction if `yt-dlp` is unavailable.
/// 3. Never plays fake synthesized audio — returns `Err(String)` if real extraction fails.
pub async fn extract_youtube_audio_stream(
    video_id: &str,
    auth: Option<&AuthSession>,
) -> Result<ExtractedAudio, String> {
    extract_youtube_audio_stream_cancellable(video_id, auth, None).await
}

pub async fn extract_youtube_audio_stream_cancellable(
    video_id: &str,
    auth: Option<&AuthSession>,
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
) -> Result<ExtractedAudio, String> {
    let clean_id = parse_youtube_video_id(video_id);

    // Step A: Primary extraction via isolated yt-dlp binary streaming progressively to RAM
    match try_fetch_yt_dlp_audio(&clean_id, auth, generation_pair.clone()).await {
        Ok(audio) => return Ok(audio),
        Err(e) if e == "Download cancelado (nova música selecionada)" => return Err(e),
        Err(yt_dlp_err) => {
            // Step B: Secondary attempt via direct Innertube HTTP client
            if let Ok(innertube_audio) = try_fetch_innertube_audio(&clean_id, auth).await {
                return Ok(innertube_audio);
            }
            return Err(yt_dlp_err);
        }
    }
}

async fn try_fetch_yt_dlp_audio(
    video_id: &str,
    auth: Option<&AuthSession>,
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
) -> Result<ExtractedAudio, String> {
    let bin_path = ensure_yt_dlp_binary().await?;

    // First attempt unauthenticated (fastest & avoids account rate-limits on public music videos)
    match run_yt_dlp_stdout_stream(&bin_path, video_id, None, generation_pair.clone()).await {
        Ok(audio) => return Ok(audio),
        Err(e) if e == "Download cancelado (nova música selecionada)" => return Err(e),
        Err(unauth_err) => {
            // If user is authenticated, retry with temporary Netscape cookies file
            if let Some(session) = auth {
                if session.is_authenticated {
                    if let Some(cookies_file) = write_temp_netscape_cookies(session) {
                        let res = run_yt_dlp_stdout_stream(
                            &bin_path,
                            video_id,
                            Some(&cookies_file),
                            generation_pair,
                        )
                        .await;
                        if res.is_err() {
                            let _ = fs::remove_file(&cookies_file);
                        }
                        if let Ok(audio) = res {
                            return Ok(audio);
                        }
                    }
                }
            }
            Err(unauth_err)
        }
    }
}

async fn run_yt_dlp_stdout_stream(
    bin_path: &PathBuf,
    video_id: &str,
    cookies_file: Option<&PathBuf>,
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
) -> Result<ExtractedAudio, String> {
    let url = format!("https://www.youtube.com/watch?v={video_id}");

    let mut cmd = Command::new(bin_path);
    cmd.arg("--no-playlist")
        .arg("--js-runtimes")
        .arg("node,deno,bun")
        .arg("-f")
        .arg("140/bestaudio[ext=m4a]/bestaudio")
        .arg("-o")
        .arg("-")
        .arg("--quiet")
        .arg("--no-warnings")
        .arg("--no-part")
        .arg("--no-cache-dir");

    if let Some(cookie_path) = cookies_file {
        cmd.arg("--cookies").arg(cookie_path);
    }

    cmd.arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Falha ao executar yt-dlp ({:?}): {e}", bin_path))?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Falha ao capturar stdout do yt-dlp".to_string())?;
    let mut stderr = child.stderr.take();

    // Cap RAM buffer at 25 MB for low-RAM footprint
    let max_bytes: usize = 25 * 1024 * 1024;
    // Initial progressive threshold: 256 KB is more than enough for DASH M4A `ftyp`+`moov`+`sidx`+first `moof`/`mdat`
    let initial_threshold: usize = 256 * 1024;
    let progressive_buffer = ProgressiveAudioBuffer::new(4 * 1024 * 1024, None);
    let mut chunk = [0u8; 32_768];
    let mut eof_reached = false;

    while progressive_buffer.len() < initial_threshold {
        if let Some((expected_gen, ref counter)) = generation_pair {
            if counter.load(Ordering::SeqCst) != expected_gen {
                let _ = child.kill().await;
                return Err("Download cancelado (nova música selecionada)".to_string());
            }
        }

        let read_res = tokio::time::timeout(Duration::from_secs(30), stdout.read(&mut chunk)).await;
        let n = match read_res {
            Ok(Ok(0)) => {
                eof_reached = true;
                break;
            }
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                let _ = child.kill().await;
                return Err(format!("Erro ao ler stream de áudio do yt-dlp: {e}"));
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err("Tempo limite excedido ao extrair áudio via yt-dlp".to_string());
            }
        };

        progressive_buffer.push_chunk(&chunk[..n]);
    }

    if eof_reached {
        progressive_buffer.mark_finished();
    }

    if progressive_buffer.len() < 16_384 {
        let status = child.wait().await.ok();
        let mut err_msg = String::new();
        if let Some(ref mut err_pipe) = stderr {
            let _ = err_pipe.read_to_string(&mut err_msg).await;
        }
        let clean_err = err_msg.trim();
        if !clean_err.is_empty() {
            return Err(format!("yt-dlp falhou para {video_id}: {clean_err}"));
        }
        return Err(format!(
            "yt-dlp retornou buffer insuficiente ({} bytes, status: {:?}) para {video_id}",
            progressive_buffer.len(),
            status.and_then(|s| s.code())
        ));
    }

    let cookies_to_cleanup = cookies_file.cloned();

    // Spawn the background chunk reader on a dedicated OS thread (`std::thread::spawn`)
    // so that even under single-threaded Tokio runtimes, background chunks are continuously
    // pumped while `Decoder::new(progressive_buffer.reader())` probes/decodes the stream!
    if !eof_reached {
        let bg_buffer = progressive_buffer.clone();
        let bg_gen = generation_pair.clone();
        let rt_handle = tokio::runtime::Handle::current();
        std::thread::Builder::new()
            .name(format!("ytm-stream-{video_id}"))
            .spawn(move || {
                rt_handle.block_on(async move {
                    let mut bg_chunk = [0u8; 32_768];
                    loop {
                        if let Some((expected_gen, ref counter)) = bg_gen {
                            if counter.load(Ordering::SeqCst) != expected_gen {
                                let _ = child.kill().await;
                                bg_buffer.mark_error(
                                    "Download cancelado (nova música selecionada)".to_string(),
                                );
                                break;
                            }
                        }

                        let read_res = tokio::time::timeout(
                            Duration::from_secs(30),
                            stdout.read(&mut bg_chunk),
                        )
                        .await;
                        match read_res {
                            Ok(Ok(0)) => {
                                bg_buffer.mark_finished();
                                break;
                            }
                            Ok(Ok(n)) => {
                                let current_len = bg_buffer.len();
                                let allowed = max_bytes.saturating_sub(current_len);
                                if n >= allowed {
                                    if allowed > 0 {
                                        bg_buffer.push_chunk(&bg_chunk[..allowed]);
                                    }
                                    let _ = child.kill().await;
                                    bg_buffer.mark_finished();
                                    break;
                                } else {
                                    bg_buffer.push_chunk(&bg_chunk[..n]);
                                }
                            }
                            Ok(Err(e)) => {
                                let _ = child.kill().await;
                                bg_buffer.mark_error(format!("Erro ao ler stream de áudio: {e}"));
                                break;
                            }
                            Err(_) => {
                                let _ = child.kill().await;
                                bg_buffer
                                    .mark_error("Timeout no streaming progressivo".to_string());
                                break;
                            }
                        }
                    }
                    let _ = child.wait().await;
                    if let Some(cookie_path) = cookies_to_cleanup {
                        let _ = fs::remove_file(cookie_path);
                    }
                });
            })
            .map_err(|e| format!("Falha ao criar thread de streaming progressivo: {e}"))?;
    } else if let Some(cookie_path) = cookies_to_cleanup {
        let _ = fs::remove_file(cookie_path);
    }

    // Fast-path: If the initial 256 KB contains a valid DASH MP4 `sidx` index box,
    // we already know the exact byte size and duration without blocking on the full stream!
    let decoded_duration = if let Some((exact_bytes, sidx_dur)) =
        progressive_buffer.inspect_mp4_sidx()
    {
        progressive_buffer.set_total_expected(exact_bytes.min(max_bytes as u64));
        sidx_dur
    } else {
        // Fallback probe for non-DASH containers
        let probe_buffer = progressive_buffer.clone();
        let vid_owned = video_id.to_string();
        tokio::task::spawn_blocking(move || {
            let decoder = Decoder::new(probe_buffer.reader()).map_err(|e| {
                format!("Falha ao decodificar container de áudio extraído para {vid_owned}: {e}")
            })?;
            let dur = decoder
                .total_duration()
                .map(|d| d.as_secs_f64())
                .filter(|&d| d > 1.0)
                .unwrap_or(0.0);
            Ok::<f64, String>(dur)
        })
        .await
        .map_err(|e| format!("Erro na thread de probe de áudio: {e}"))??
    };

    let quality_label = if cookies_file.is_some() {
        "128 kbps M4A (yt-dlp • Streaming Progressivo Autenticado)".to_string()
    } else {
        "128 kbps M4A (yt-dlp • Streaming Progressivo em Chunks)".to_string()
    };

    Ok(ExtractedAudio {
        data: progressive_buffer,
        duration_secs: decoded_duration,
        quality_label,
        title: None,
        author: None,
    })
}

struct ClientProfile {
    name: &'static str,
    version: &'static str,
    user_agent: &'static str,
    client_id: &'static str,
    origin: &'static str,
    endpoint_url: &'static str,
    extra_context: Option<Value>,
}

async fn try_fetch_innertube_audio(
    video_id: &str,
    auth: Option<&AuthSession>,
) -> Result<ExtractedAudio, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| format!("Erro ao criar cliente HTTP: {e}"))?;

    let profiles = [
        ClientProfile {
            name: "ANDROID_VR",
            version: "1.60.19",
            user_agent: "com.google.android.apps.youtube.vr.oculus/1.60.19 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
            client_id: "28",
            origin: "https://www.youtube.com",
            endpoint_url: "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
            extra_context: Some(json!({
                "androidSdkVersion": 32,
                "osName": "Android",
                "osVersion": "12L"
            })),
        },
        ClientProfile {
            name: "TVHTML5_SIMPLY_EMBEDDED_PLAYER",
            version: "2.0",
            user_agent: "Mozilla/5.0 (PlayStation; PlayStation 5/2.26) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/13.0 Safari/605.1.15",
            client_id: "85",
            origin: "https://www.youtube.com",
            endpoint_url: "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
            extra_context: None,
        },
        ClientProfile {
            name: "WEB_REMIX",
            version: "1.20250101.01.00",
            user_agent: FIREFOX_LINUX_UA,
            client_id: "67",
            origin: YTM_ORIGIN,
            endpoint_url: "https://music.youtube.com/youtubei/v1/player?prettyPrint=false",
            extra_context: Some(json!({
                "platform": "DESKTOP",
                "osName": "Linux",
                "osVersion": "x86_64"
            })),
        },
        ClientProfile {
            name: "IOS",
            version: "19.45.4",
            user_agent: "com.google.ios.youtube/19.45.4 (iPhone16,2; U; CPU iOS 18_1_0 like Mac OS X;)",
            client_id: "5",
            origin: "https://www.youtube.com",
            endpoint_url: "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
            extra_context: Some(json!({
                "deviceMake": "Apple",
                "deviceModel": "iPhone16,2",
                "osName": "iPhone",
                "osVersion": "18.1.0.22B83"
            })),
        },
        ClientProfile {
            name: "ANDROID",
            version: "19.44.38",
            user_agent: "com.google.android.youtube/19.44.38 (Linux; U; Android 14) gzip",
            client_id: "3",
            origin: "https://www.youtube.com",
            endpoint_url: "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
            extra_context: Some(json!({
                "androidSdkVersion": 34,
                "osName": "Android",
                "osVersion": "14"
            })),
        },
    ];

    for profile in profiles {
        let mut client_obj = json!({
            "clientName": profile.name,
            "clientVersion": profile.version,
            "hl": "pt",
            "gl": "BR",
        });

        if let (Some(obj), Some(extra)) = (client_obj.as_object_mut(), profile.extra_context) {
            if let Some(extra_map) = extra.as_object() {
                for (k, v) in extra_map {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        let body = json!({
            "videoId": video_id,
            "contentCheckOk": true,
            "racyCheckOk": true,
            "context": {
                "client": client_obj,
                "thirdParty": {
                    "embedUrl": "https://www.youtube.com/"
                }
            }
        });

        let mut req = client
            .post(profile.endpoint_url)
            .header("User-Agent", profile.user_agent)
            .header("X-YouTube-Client-Name", profile.client_id)
            .header("X-YouTube-Client-Version", profile.version)
            .header("Origin", profile.origin)
            .header("X-Origin", profile.origin);

        if let Some(session) = auth {
            if session.is_authenticated && !session.sapisid.is_empty() {
                let sapisidhash = generate_sapisidhash(&session.sapisid, profile.origin);
                req = req
                    .header("Cookie", &session.cookie_header)
                    .header("Authorization", sapisidhash)
                    .header("X-Goog-AuthUser", "0");
            }
        }

        let response = req.json(&body).send().await;

        let Ok(resp) = response else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }

        let Ok(json_data) = resp.json::<Value>().await else {
            continue;
        };

        let title = json_data
            .pointer("/videoDetails/title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let author = json_data
            .pointer("/videoDetails/author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let duration_secs = json_data
            .pointer("/videoDetails/lengthSeconds")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(180.0);

        let Some(adaptive_formats) = json_data
            .pointer("/streamingData/adaptiveFormats")
            .and_then(|v| v.as_array())
        else {
            continue;
        };

        let mut best_url: Option<String> = None;
        let mut best_score: i32 = -1;

        for fmt in adaptive_formats {
            let Some(url) = fmt.get("url").and_then(|u| u.as_str()) else {
                continue;
            };
            let mime = fmt.get("mimeType").and_then(|m| m.as_str()).unwrap_or("");
            if !mime.starts_with("audio/") {
                continue;
            }
            let itag = fmt.get("itag").and_then(|i| i.as_i64()).unwrap_or(0);

            let score = if itag == 140 {
                100
            } else if mime.starts_with("audio/mp4") {
                80
            } else if mime.starts_with("audio/webm") || mime.starts_with("audio/ogg") {
                50
            } else {
                20
            };

            if score > best_score {
                best_score = score;
                best_url = Some(url.to_string());
            }
        }

        let Some(stream_url) = best_url else {
            continue;
        };

        let max_bytes: usize = 25 * 1024 * 1024;
        let mut get_req = client
            .get(&stream_url)
            .header("User-Agent", profile.user_agent)
            .header("Origin", profile.origin)
            .header("Range", format!("bytes=0-{}", max_bytes - 1));

        if let Some(session) = auth {
            if session.is_authenticated && !session.cookie_header.is_empty() {
                get_req = get_req.header("Cookie", &session.cookie_header);
            }
        }

        let Ok(audio_resp) = get_req.send().await else {
            continue;
        };

        if !audio_resp.status().is_success() && audio_resp.status().as_u16() != 206 {
            continue;
        }

        if let Ok(bytes) = audio_resp.bytes().await {
            if bytes.len() > 16_384 {
                let mut vec_data = bytes.to_vec();
                if vec_data.len() > max_bytes {
                    vec_data.truncate(max_bytes);
                }
                let prog_buf = ProgressiveAudioBuffer::from_vec(vec_data);
                if Decoder::new(prog_buf.reader()).is_ok() {
                    let quality_label = if auth.is_some_and(|a| a.is_authenticated) {
                        format!("128 kbps M4A ({} • Autenticado na RAM)", profile.name)
                    } else {
                        format!("128 kbps M4A ({} • YouTube Stream RAM)", profile.name)
                    };
                    return Ok(ExtractedAudio {
                        data: prog_buf,
                        duration_secs: duration_secs.max(1.0),
                        quality_label,
                        title,
                        author,
                    });
                }
            }
        }
    }

    Err("Não foi possível extrair o stream de áudio real do YouTube".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_real_youtube_audio_extraction_and_decoding() {
        let bin_path = ensure_yt_dlp_binary()
            .await
            .expect("yt-dlp binary should be present or downloaded");
        assert!(bin_path.exists(), "yt-dlp binary must exist on disk");

        // Extract two distinct real YouTube songs (progressive buffer)
        let audio1 = extract_youtube_audio_stream("dQw4w9WgXcQ", None)
            .await
            .expect("First real YouTube track (dQw4w9WgXcQ) should extract successfully");
        let audio2 = extract_youtube_audio_stream("fJ9rUzIMcZQ", None)
            .await
            .expect("Second real YouTube track (fJ9rUzIMcZQ) should extract successfully");

        assert!(
            audio1.data.len() > 50_000,
            "Audio 1 initial progressive bytes should be > 50KB, got {}",
            audio1.data.len()
        );
        assert!(
            audio2.data.len() > 50_000,
            "Audio 2 initial progressive bytes should be > 50KB, got {}",
            audio2.data.len()
        );
        assert_ne!(
            audio1.data, audio2.data,
            "Two different YouTube video IDs must produce distinct audio buffers"
        );

        // Decode on a blocking thread while the dedicated background threads pump stdout!
        let buf1 = audio1.data.clone();
        let buf2 = audio2.data.clone();
        let (samples_1, samples_2) = tokio::task::spawn_blocking(move || {
            let mut dec1 = Decoder::new(buf1.reader())
                .expect("rodio::Decoder must decode real M4A progressive stream 1");
            let mut dec2 = Decoder::new(buf2.reader())
                .expect("rodio::Decoder must decode real M4A progressive stream 2");

            assert!(dec1.channels() > 0 && dec1.sample_rate() > 0);
            assert!(dec2.channels() > 0 && dec2.sample_rate() > 0);

            // Decode > 30 seconds of samples (well past the initial 256 KB chunk!)
            let s1: usize = (&mut dec1).take(44_100 * 2 * 35).count();
            let s2: usize = (&mut dec2).take(44_100 * 2 * 35).count();
            (s1, s2)
        })
        .await
        .expect("spawn_blocking decoding must succeed");

        assert!(
            samples_1 >= 44_100 * 2 * 30,
            "Must decode >30s of samples from progressive stream 1, got {samples_1}"
        );
        assert!(
            samples_2 >= 44_100 * 2 * 30,
            "Must decode >30s of samples from progressive stream 2, got {samples_2}"
        );
    }

    #[test]
    fn test_progressive_audio_buffer_and_reader_concurrent_streaming() {
        let buffer = ProgressiveAudioBuffer::new(1024, Some(10));
        assert!(buffer.is_empty());
        assert!(!buffer.is_finished());

        let writer_buf = buffer.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            writer_buf.push_chunk(&[1, 2, 3, 4, 5]);
            std::thread::sleep(Duration::from_millis(30));
            writer_buf.push_chunk(&[6, 7, 8, 9, 10]);
            writer_buf.mark_finished();
        });

        let mut reader = buffer.reader();
        let mut out = Vec::new();
        reader
            .read_to_end(&mut out)
            .expect("ProgressiveReader must read all chunks until mark_finished");
        handle.join().unwrap();

        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert!(buffer.is_finished());
        assert_eq!(buffer.len(), 10);
    }
}
