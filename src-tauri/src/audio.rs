use crate::auth::{generate_sapisidhash, AuthSession, FIREFOX_LINUX_UA, YTM_ORIGIN};
use rodio::{Decoder, OutputStream, Sink, Source};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
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
use tokio::sync::Notify;

pub const YT_DLP_LINUX_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";
pub const YT_DLP_LINUX_AARCH64_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64";
pub const YT_DLP_WINDOWS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
pub const YT_DLP_MACOS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";
pub const YT_DLP_SHA256SUMS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";

const MAX_YT_DLP_BINARY_BYTES: usize = 100 * 1024 * 1024;
const MAX_SHA256SUMS_BYTES: usize = 256 * 1024;
const MAX_YT_DLP_STDERR_BYTES: u64 = 8_192;
const CANCELLED_ERR_MSG: &str = "Download cancelado (nova música selecionada)";

/// Sentinel returned by `ProgressiveReader::seek(SeekFrom::End(0))` when a progressive stream
/// is still in flight and does not have an upfront total byte length (e.g. non-DASH container).
/// `ReadSeekSource::new()` maps this sentinel to `byte_len = None` so Symphonia never seeks
/// to a fake EOF offset or blocks waiting for the entire network stream before probing headers.
pub const UNKNOWN_PROGRESSIVE_END_SENTINEL: u64 = u64::MAX / 4;

/// Returns the official yt-dlp standalone release URL for the current OS and architecture.
pub fn get_yt_dlp_download_url() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        YT_DLP_WINDOWS_URL
    }
    #[cfg(target_os = "macos")]
    {
        YT_DLP_MACOS_URL
    }
    #[cfg(all(
        not(target_os = "windows"),
        not(target_os = "macos"),
        target_arch = "aarch64"
    ))]
    {
        YT_DLP_LINUX_AARCH64_URL
    }
    #[cfg(all(
        not(target_os = "windows"),
        not(target_os = "macos"),
        not(target_arch = "aarch64")
    ))]
    {
        YT_DLP_LINUX_URL
    }
}

/// Returns the yt-dlp executable filename for the current OS.
pub fn get_yt_dlp_binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "yt-dlp.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "yt-dlp"
    }
}

/// Validates redirect targets when downloading the official yt-dlp release from GitHub.
fn is_trusted_yt_dlp_download_url(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    match url.host_str() {
        Some(host) => {
            host == "github.com"
                || host.ends_with(".github.com")
                || host == "objects.githubusercontent.com"
                || host == "release-assets.githubusercontent.com"
                || host == "github-releases.githubusercontent.com"
        }
        None => false,
    }
}

static COOKIE_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
static YT_DLP_INIT_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Parses the ISO-BMFF / MP4 `sidx` (Segment Index) box located in the initial header (~2 KB)
/// of YouTube DASH M4A (`itag 140`) streams to determine the exact total byte length and
/// exact duration in seconds before the rest of the stream has finished downloading.
pub fn parse_mp4_sidx_metadata(data: &[u8]) -> Option<(u64, f64)> {
    let mut pos: usize = 0;
    while pos
        .checked_add(8)
        .is_some_and(|hdr_end| hdr_end <= data.len())
    {
        let size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let box_type = &data[pos + 4..pos + 8];
        if size < 8 {
            break;
        }
        let Some(box_end) = pos.checked_add(size) else {
            break;
        };
        if box_type == b"sidx" {
            if box_end > data.len() || size < 32 {
                return None;
            }
            let sidx_end = box_end as u64;
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
                (fo, pos.checked_add(28)?)
            } else {
                let v1_min_end = pos.checked_add(40)?;
                if v1_min_end > data.len() {
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
                (fo, pos.checked_add(36)?)
            };

            let ref_hdr_end = ref_pos.checked_add(4)?;
            if ref_hdr_end > box_end {
                return None;
            }
            let ref_count = u16::from_be_bytes([data[ref_pos + 2], data[ref_pos + 3]]) as usize;
            let mut cur = ref_hdr_end;
            let entries_bytes = ref_count.checked_mul(12)?;
            let entries_end = cur.checked_add(entries_bytes)?;
            if entries_end > box_end {
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
                total_ref_size = total_ref_size.saturating_add((raw_size & 0x7FFF_FFFF) as u64);
                total_dur_ticks = total_dur_ticks.saturating_add(sub_dur as u64);
                cur = cur.checked_add(12)?;
            }

            let exact_total_bytes = sidx_end
                .saturating_add(first_offset)
                .saturating_add(total_ref_size);
            let duration_secs = if timescale > 0 {
                (total_dur_ticks as f64) / (timescale as f64)
            } else {
                0.0
            };
            return Some((exact_total_bytes, duration_secs));
        }

        if box_end > data.len() {
            break;
        }
        pos = box_end;
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
    /// Fast-path: Only scans for DASH MP4 `sidx` box in the initial 64 KB header.
    pub fn push_chunk(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.shared.state.lock() {
            guard.data.extend_from_slice(chunk);
            if guard.total_expected.is_none() && guard.data.len() <= 65_536 {
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

    /// Marks the progressive stream as completely downloaded, shrinks excess `Vec` capacity, and wakes all readers.
    pub fn mark_finished(&self) {
        if let Ok(mut guard) = self.shared.state.lock() {
            guard.finished = true;
            guard.data.shrink_to_fit();
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
                    } else if delta != 0 {
                        // Only wait for stream completion if a caller is explicitly seeking
                        // to a negative offset relative to the end (`SeekFrom::End(-N)`).
                        // When `delta == 0` (`ReadSeekSource::new()` probing `byte_len()`),
                        // return `UNKNOWN_PROGRESSIVE_END_SENTINEL` immediately so non-DASH
                        // streams (MP3, OGG, WAV, FLAC) start playing without blocking!
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
                    state.total_expected.unwrap_or_else(|| {
                        if delta == 0 {
                            UNKNOWN_PROGRESSIVE_END_SENTINEL
                        } else {
                            state.data.len() as u64
                        }
                    })
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

        let target_u64 = u64::try_from(target).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Deslocamento de seek fora dos limites válidos",
            )
        })?;
        let target_usize = usize::try_from(target_u64).unwrap_or(usize::MAX);

        // If a decoder/demuxer attempts to seek to a specific byte offset beyond the currently
        // downloaded data, wait on the Condvar until `data.len() >= target` or the stream finishes.
        if !matches!(style, SeekFrom::End(_)) {
            while target_usize > state.data.len() && !state.finished {
                let (new_state, timeout_res) = self
                    .buffer
                    .shared
                    .cvar
                    .wait_timeout(state, Duration::from_secs(20))
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "Condvar wait poisoned"))?;
                state = new_state;
                if timeout_res.timed_out() && target_usize > state.data.len() && !state.finished {
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
    pub last_error: Mutex<Option<String>>,
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
            last_error: Mutex::new(None),
        }
    }

    pub fn position_seconds(&self) -> f64 {
        (self.position_ms.load(Ordering::Relaxed) as f64) / 1000.0
    }
}

/// Commands sent from Tauri IPC / System Tray threads to the dedicated native audio thread.
pub enum AudioCommand {
    LoadAndPlay {
        data: ProgressiveAudioBuffer,
        volume: f32,
        start_paused: bool,
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
                let mut stream_opt = OutputStream::try_default().ok();
                let mut sink_opt: Option<Sink> = None;
                let mut current_buffer: Option<ProgressiveAudioBuffer> = None;
                let mut current_volume: f32 = 0.8;
                let mut is_paused: bool = false;
                let mut seek_offset_secs: f64 = 0.0;

                loop {
                    // When idle or paused, block on `rx.recv()` for 0 CPU wakeups/sec.
                    // Only poll at 100ms intervals while actively playing to update position telemetry.
                    let recv_res = if sink_opt.is_none() || is_paused {
                        rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
                    } else {
                        rx.recv_timeout(Duration::from_millis(100))
                    };

                    match recv_res {
                        Ok(cmd) => match cmd {
                            AudioCommand::LoadAndPlay {
                                data,
                                volume,
                                start_paused,
                            } => {
                                current_volume = volume.clamp(0.0, 1.0);
                                is_paused = start_paused;
                                seek_offset_secs = 0.0;

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
                                    .store(false, Ordering::Relaxed);
                                if let Ok(mut err_guard) = thread_telemetry.last_error.lock() {
                                    *err_guard = None;
                                }

                                let mut init_error: Option<String> = None;
                                if stream_opt.is_none() {
                                    match OutputStream::try_default() {
                                        Ok(stream) => stream_opt = Some(stream),
                                        Err(e) => {
                                            init_error = Some(format!(
                                                "Nenhum dispositivo de saída de áudio disponível no sistema ({e})."
                                            ));
                                        }
                                    }
                                }

                                // Try creating a Sink on the active OutputStream; if the OS audio
                                // endpoint changed or disconnected (common on Windows WASAPI when
                                // switching headphones/Bluetooth/HDMI), re-initialize OutputStream once!
                                let new_sink_res = if let Some((_, ref stream_handle)) = stream_opt
                                {
                                    match Sink::try_new(stream_handle) {
                                        Ok(sink) => Ok(sink),
                                        Err(_) => {
                                            stream_opt = OutputStream::try_default().ok();
                                            if let Some((_, ref fresh_handle)) = stream_opt {
                                                Sink::try_new(fresh_handle).map_err(|e| {
                                                    format!(
                                                        "Falha ao abrir canal de saída de áudio: {e}"
                                                    )
                                                })
                                            } else {
                                                Err(
                                                    "Dispositivo de saída de áudio desconectado ou indisponível."
                                                        .to_string(),
                                                )
                                            }
                                        }
                                    }
                                } else {
                                    Err(init_error.take().unwrap_or_else(|| {
                                        "Nenhum dispositivo de saída de áudio disponível no sistema."
                                            .to_string()
                                    }))
                                };

                                match new_sink_res {
                                    Ok(new_sink) => {
                                        new_sink.set_volume(current_volume);
                                        match Decoder::new(data.reader()) {
                                            Ok(decoder) => {
                                                new_sink.append(decoder);
                                                if start_paused {
                                                    new_sink.pause();
                                                } else {
                                                    new_sink.play();
                                                }
                                                sink_opt = Some(new_sink);
                                                thread_telemetry
                                                    .has_active_track
                                                    .store(true, Ordering::Relaxed);
                                            }
                                            Err(e) => {
                                                init_error = Some(format!(
                                                    "Falha ao decodificar fluxo de áudio: {e}"
                                                ));
                                            }
                                        }
                                    }
                                    Err(err_msg) => {
                                        init_error = Some(err_msg);
                                    }
                                }

                                if let Some(err_msg) = init_error {
                                    thread_telemetry
                                        .is_sink_empty
                                        .store(true, Ordering::Relaxed);
                                    if let Ok(mut err_guard) = thread_telemetry.last_error.lock() {
                                        *err_guard = Some(err_msg);
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

                                if !seek_succeeded {
                                    if stream_opt.is_none() {
                                        stream_opt = OutputStream::try_default().ok();
                                    }
                                    if let (Some((_, ref stream_handle)), Some(ref buffer)) =
                                        (&stream_opt, &current_buffer)
                                    {
                                        if let Some(old_sink) = sink_opt.take() {
                                            old_sink.stop();
                                        }
                                        let mut seek_err: Option<String> = None;
                                        match Sink::try_new(stream_handle) {
                                            Ok(new_sink) => {
                                                new_sink.set_volume(current_volume);
                                                match Decoder::new(buffer.reader()) {
                                                    Ok(mut decoder) => {
                                                        if decoder.try_seek(target).is_ok() {
                                                            seek_offset_secs = clamped_secs;
                                                            new_sink.append(decoder);
                                                            if is_paused {
                                                                new_sink.pause();
                                                            } else {
                                                                new_sink.play();
                                                            }
                                                            sink_opt = Some(new_sink);
                                                        } else {
                                                            match Decoder::new(buffer.reader()) {
                                                                Ok(dec_skip) => {
                                                                    seek_offset_secs = clamped_secs;
                                                                    new_sink.append(
                                                                        dec_skip
                                                                            .skip_duration(target),
                                                                    );
                                                                    if is_paused {
                                                                        new_sink.pause();
                                                                    } else {
                                                                        new_sink.play();
                                                                    }
                                                                    sink_opt = Some(new_sink);
                                                                }
                                                                Err(e) => {
                                                                    seek_err = Some(format!(
                                                                        "Falha ao reposicionar decodificador de áudio: {e}"
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        seek_err = Some(format!(
                                                            "Falha ao reabrir decodificador de áudio durante seek: {e}"
                                                        ));
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                seek_err = Some(format!(
                                                    "Falha ao recriar canal de saída de áudio durante seek: {e}"
                                                ));
                                            }
                                        }

                                        if let Some(err_msg) = seek_err {
                                            thread_telemetry
                                                .has_active_track
                                                .store(false, Ordering::Relaxed);
                                            thread_telemetry
                                                .is_sink_empty
                                                .store(true, Ordering::Relaxed);
                                            if let Ok(mut err_guard) =
                                                thread_telemetry.last_error.lock()
                                            {
                                                *err_guard = Some(err_msg);
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
                                is_paused = false;
                                seek_offset_secs = 0.0;
                                thread_telemetry.position_ms.store(0, Ordering::Relaxed);
                                thread_telemetry
                                    .is_sink_empty
                                    .store(true, Ordering::Relaxed);
                                thread_telemetry
                                    .has_active_track
                                    .store(false, Ordering::Relaxed);
                            }
                        },
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }

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
        match &cmd {
            AudioCommand::LoadAndPlay { .. } => {
                self.telemetry.position_ms.store(0, Ordering::Relaxed);
                self.telemetry.is_sink_empty.store(false, Ordering::Relaxed);
                self.telemetry
                    .has_active_track
                    .store(false, Ordering::Relaxed);
            }
            AudioCommand::Seek(_) => {
                self.telemetry.is_sink_empty.store(false, Ordering::Relaxed);
            }
            AudioCommand::Stop => {
                self.telemetry.is_sink_empty.store(true, Ordering::Relaxed);
                self.telemetry
                    .has_active_track
                    .store(false, Ordering::Relaxed);
            }
            _ => {}
        }
        if let Ok(guard) = self.sender.lock() {
            let _ = guard.send(cmd);
        }
    }

    pub fn get_position_secs(&self) -> f64 {
        self.telemetry.position_seconds()
    }

    pub fn has_active_track(&self) -> bool {
        self.telemetry.has_active_track.load(Ordering::Relaxed)
    }

    pub fn take_last_error(&self) -> Option<String> {
        self.telemetry
            .last_error
            .lock()
            .ok()
            .and_then(|mut g| g.take())
    }

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
    pub cancel_flag: Option<Arc<AtomicBool>>,
}

/// Resolves `<app_data_dir>/bin/yt-dlp` (or `yt-dlp.exe` on Windows).
pub fn get_yt_dlp_bin_path() -> PathBuf {
    crate::persistence::get_app_data_dir()
        .join("bin")
        .join(get_yt_dlp_binary_name())
}

/// Resolves `<app_data_dir>` for temporary Netscape cookie files.
pub fn get_temp_cookie_dir() -> PathBuf {
    crate::persistence::get_app_data_dir()
}

/// Sweeps and removes any orphaned temporary cookie files left behind by interrupted sessions.
pub fn clean_temp_cookie_files() {
    let dir = get_temp_cookie_dir();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with("cookies.") && file_name.ends_with(".tmp.txt") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn parse_sha256sums_for_asset(sums_text: &str, asset_name: &str) -> Option<String> {
    for line in sums_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let (Some(hash), Some(raw_name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let clean_name = raw_name.trim_start_matches('*').trim_start_matches("./");
        if clean_name == asset_name
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

async fn read_response_bytes_bounded(
    mut resp: reqwest::Response,
    max_bytes: usize,
    label: &str,
) -> Result<Vec<u8>, String> {
    if let Some(content_len) = resp.content_length() {
        if content_len > max_bytes as u64 {
            return Err(format!(
                "{label} excedeu o limite seguro de {max_bytes} bytes ({content_len} bytes)"
            ));
        }
    }

    let initial_cap = resp
        .content_length()
        .map(|n| (n as usize).min(max_bytes))
        .unwrap_or(64 * 1024);
    let mut out = Vec::with_capacity(initial_cap);

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Erro ao ler bytes de {label}: {e}"))?
    {
        if out.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!(
                "{label} excedeu o limite seguro de {max_bytes} bytes"
            ));
        }
        out.extend_from_slice(&chunk);
    }

    Ok(out)
}

/// Ensures the standalone `yt-dlp` binary exists at `<app_data_dir>/bin/yt-dlp` (`yt-dlp.exe` on Windows).
/// If not present on disk, downloads the official GitHub release binary + `SHA2-256SUMS`,
/// verifies the SHA-256 digest before persisting, and sets executable permissions (`0755` on Unix).
/// Protected by an async mutex so concurrent calls never race.
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
                    if let Some(parent) = bin_path.parent() {
                        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
                    }
                    let _ = fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755));
                }
                return Ok(bin_path);
            }
        }
    }

    if let Some(parent) = bin_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Falha ao criar diretório para yt-dlp: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let download_url = get_yt_dlp_download_url();
    let asset_name = download_url
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "URL de download do yt-dlp inválida".to_string())?;

    let parsed_sums_url = reqwest::Url::parse(YT_DLP_SHA256SUMS_URL)
        .map_err(|e| format!("URL de SHA2-256SUMS inválida: {e}"))?;
    let parsed_bin_url = reqwest::Url::parse(download_url)
        .map_err(|e| format!("URL do binário yt-dlp inválida: {e}"))?;
    if !is_trusted_yt_dlp_download_url(&parsed_sums_url)
        || !is_trusted_yt_dlp_download_url(&parsed_bin_url)
    {
        return Err("URL de download do yt-dlp não confiável".to_string());
    }

    let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("Muitos redirecionamentos ao baixar yt-dlp");
        }
        if is_trusted_yt_dlp_download_url(attempt.url()) {
            attempt.follow()
        } else {
            attempt.error("Redirecionamento não confiável ao baixar yt-dlp")
        }
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(redirect_policy)
        .build()
        .map_err(|e| format!("Falha ao inicializar cliente HTTP para yt-dlp: {e}"))?;

    // 1. Fetch official SHA2-256SUMS from the same GitHub release (bounded to 256 KB)
    let sums_resp = client
        .get(YT_DLP_SHA256SUMS_URL)
        .header("User-Agent", FIREFOX_LINUX_UA)
        .send()
        .await
        .map_err(|e| format!("Falha ao baixar SHA2-256SUMS oficial do yt-dlp: {e}"))?;

    if !sums_resp.status().is_success() {
        return Err(format!(
            "Download de SHA2-256SUMS do yt-dlp retornou HTTP {} (verificação obrigatória)",
            sums_resp.status()
        ));
    }

    let sums_bytes =
        read_response_bytes_bounded(sums_resp, MAX_SHA256SUMS_BYTES, "SHA2-256SUMS").await?;
    let sums_text = String::from_utf8(sums_bytes)
        .map_err(|e| format!("SHA2-256SUMS do yt-dlp não é UTF-8 válido: {e}"))?;
    let expected_sha256 = parse_sha256sums_for_asset(&sums_text, asset_name).ok_or_else(|| {
        format!("Checksum SHA-256 para '{asset_name}' não encontrado em SHA2-256SUMS")
    })?;

    // 2. Download the standalone yt-dlp binary bounded to 100 MB
    let resp = client
        .get(download_url)
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

    let bytes =
        read_response_bytes_bounded(resp, MAX_YT_DLP_BINARY_BYTES, "binário yt-dlp").await?;

    if bytes.len() < 1_000_000 {
        return Err("Binário yt-dlp corrompido ou incompleto baixado do GitHub".to_string());
    }

    // 3. Verify SHA-256 integrity before writing to disk
    let actual_sha256 = sha256_hex(&bytes);
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "Falha na verificação SHA-256 do binário yt-dlp ({asset_name}): esperado {expected_sha256}, obtido {actual_sha256}"
        ));
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = PathBuf::from(format!(
        "{}.{}.{}.part",
        bin_path.display(),
        std::process::id(),
        nanos
    ));

    let write_res: Result<(), String> = (|| {
        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o755)
                .open(&tmp_path)
                .map_err(|e| format!("Falha ao criar arquivo temporário do yt-dlp: {e}"))?;
            file.write_all(&bytes)
                .map_err(|e| format!("Falha ao gravar bytes do yt-dlp: {e}"))?;
        }

        #[cfg(not(unix))]
        {
            fs::write(&tmp_path, &bytes)
                .map_err(|e| format!("Falha ao salvar yt-dlp temporário: {e}"))?;
        }

        #[cfg(windows)]
        {
            if bin_path.exists() {
                let _ = fs::remove_file(&bin_path);
            }
        }

        fs::rename(&tmp_path, &bin_path).map_err(|e| format!("Falha ao mover yt-dlp: {e}"))?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    Ok(bin_path)
}

/// Parses a YouTube or YouTube Music URL (or raw video ID) into a clean 11-character video ID.
pub fn parse_youtube_video_id(input: &str) -> String {
    let trimmed = input.trim();

    // Check standard ?v=<id> parameter
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
pub fn write_temp_netscape_cookies(session: &AuthSession) -> Option<PathBuf> {
    if !session.is_authenticated || session.cookie_header.trim().is_empty() {
        return None;
    }

    let seq = COOKIE_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();

    let parent_dir = get_temp_cookie_dir();
    let cookies_path = parent_dir.join(format!("cookies.{pid}.{nanos}.{seq}.tmp.txt"));

    if fs::create_dir_all(&parent_dir).is_err() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&parent_dir, fs::Permissions::from_mode(0o700));
    }

    let mut lines = vec![
        "# Netscape HTTP Cookie File".to_string(),
        "# Generated in-app by youtube-music-rust".to_string(),
    ];

    let sanitized_header = session.cookie_header.replace('\r', "").replace('\n', "");
    for pair in sanitized_header.split(';') {
        let trimmed = pair.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            let name = k.trim().replace('\t', "");
            let val = v.trim().replace('\t', "");
            if !name.is_empty() && !val.is_empty() {
                lines.push(format!(
                    ".youtube.com\tTRUE\t/\tTRUE\t2147483647\t{name}\t{val}"
                ));
            }
        }
    }

    let content = lines.join("\n");

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&cookies_path)
        {
            if f.write_all(content.as_bytes()).is_ok() {
                return Some(cookies_path);
            }
        }
        None
    }

    #[cfg(not(unix))]
    {
        if fs::write(&cookies_path, content).is_ok() {
            Some(cookies_path)
        } else {
            None
        }
    }
}

/// RAII guard ensuring temporary Netscape cookie files are always removed once no longer needed
/// by the `yt-dlp` subprocess (with sole ownership transferred to the background pump task when active).
struct CookieFileGuard(Option<PathBuf>);

impl CookieFileGuard {
    fn as_ref(&self) -> Option<&PathBuf> {
        self.0.as_ref()
    }

    fn take(&mut self) -> Option<PathBuf> {
        self.0.take()
    }
}

impl Drop for CookieFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn is_stream_cancelled(
    generation_pair: &Option<(u64, Arc<AtomicU64>)>,
    cancel_flag: &Option<Arc<AtomicBool>>,
    probe_abort: Option<&Arc<AtomicBool>>,
) -> bool {
    if let Some((expected_gen, counter)) = generation_pair {
        if counter.load(Ordering::SeqCst) != *expected_gen {
            return true;
        }
    }
    if let Some(flag) = cancel_flag {
        if flag.load(Ordering::SeqCst) {
            return true;
        }
    }
    if let Some(abort) = probe_abort {
        if abort.load(Ordering::SeqCst) {
            return true;
        }
    }
    false
}

fn sanitize_yt_dlp_stderr(raw: &str, cookie_path: Option<&PathBuf>) -> String {
    let mut sanitized = raw.to_string();
    if let Some(cp) = cookie_path {
        let cp_str = cp.to_string_lossy();
        if !cp_str.is_empty() {
            sanitized = sanitized.replace(cp_str.as_ref(), "[cookies]");
        }
    }
    let cookie_dir = get_temp_cookie_dir();
    let dir_str = cookie_dir.to_string_lossy();
    if !dir_str.is_empty() {
        sanitized = sanitized.replace(dir_str.as_ref(), "[app-data]");
    }
    let collapsed = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(300).collect()
}

/// Primary extraction engine:
/// 1. Uses the isolated `<app_data_dir>/bin/yt-dlp` binary to stream real YouTube
///    `m4a` (`itag 140` / `bestaudio[ext=m4a]`) bytes directly to `stdout` into a progressive RAM
///    `ProgressiveAudioBuffer`. As soon as the initial 256 KB header (`ftyp` + `moov` + `sidx` + first
///    `moof`/`mdat` audio frames) is buffered and verified by `rodio::Decoder`, returns immediately
///    while a background task continues feeding the remaining chunks!
///    Supports immediate cancellation via `generation_pair` and/or per-stream `cancel_flag` (`kill_on_drop(true)`).
/// 2. Falls back to direct Innertube `/youtubei/v1/player` extraction if `yt-dlp` is unavailable.
/// 3. Never plays fake synthesized audio — returns `Err(String)` if real extraction fails.
pub async fn extract_youtube_audio_stream(
    video_id: &str,
    auth: Option<&AuthSession>,
) -> Result<ExtractedAudio, String> {
    extract_youtube_audio_stream_internal(video_id, auth, None, None).await
}

pub async fn extract_youtube_audio_stream_cancellable(
    video_id: &str,
    auth: Option<&AuthSession>,
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
) -> Result<ExtractedAudio, String> {
    extract_youtube_audio_stream_internal(video_id, auth, generation_pair, None).await
}

pub async fn extract_youtube_audio_stream_with_cancel(
    video_id: &str,
    auth: Option<&AuthSession>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<ExtractedAudio, String> {
    extract_youtube_audio_stream_internal(video_id, auth, None, cancel_flag).await
}

async fn extract_youtube_audio_stream_internal(
    video_id: &str,
    auth: Option<&AuthSession>,
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<ExtractedAudio, String> {
    if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
        return Err(CANCELLED_ERR_MSG.to_string());
    }

    let clean_id = parse_youtube_video_id(video_id);

    // Step A: Primary extraction via isolated yt-dlp binary streaming progressively to RAM
    match try_fetch_yt_dlp_audio(
        &clean_id,
        auth,
        generation_pair.clone(),
        cancel_flag.clone(),
    )
    .await
    {
        Ok(audio) => Ok(audio),
        Err(e) if e == CANCELLED_ERR_MSG => Err(e),
        Err(yt_dlp_err) => {
            if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
                return Err(CANCELLED_ERR_MSG.to_string());
            }
            // Step B: Secondary attempt via direct Innertube HTTP client
            match try_fetch_innertube_audio(&clean_id, auth, generation_pair, cancel_flag).await {
                Ok(innertube_audio) => Ok(innertube_audio),
                Err(e) if e == CANCELLED_ERR_MSG => Err(e),
                Err(_) => Err(yt_dlp_err),
            }
        }
    }
}

async fn try_fetch_yt_dlp_audio(
    video_id: &str,
    auth: Option<&AuthSession>,
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<ExtractedAudio, String> {
    let bin_path = ensure_yt_dlp_binary().await?;

    if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
        return Err(CANCELLED_ERR_MSG.to_string());
    }

    let is_auth = auth.is_some_and(|s| s.is_authenticated);

    // If user is authenticated, use cookies directly to prevent 1.5-4s penalty waiting for unauthenticated failure.
    // Ownership of `cookies_file` is transferred into `run_yt_dlp_stdout_stream` so it is only deleted once
    // `yt-dlp` has finished (never racing with an active background pump task).
    if is_auth {
        if let Some(session) = auth {
            if let Some(cookies_file) = write_temp_netscape_cookies(session) {
                let res = run_yt_dlp_stdout_stream(
                    &bin_path,
                    video_id,
                    Some(cookies_file),
                    generation_pair.clone(),
                    cancel_flag.clone(),
                )
                .await;
                match res {
                    Ok(audio) => return Ok(audio),
                    Err(e) if e == CANCELLED_ERR_MSG => return Err(e),
                    Err(_) => {} // Fallback to unauthenticated below if cookies failed
                }
            }
        }
    }

    if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
        return Err(CANCELLED_ERR_MSG.to_string());
    }

    // Unauthenticated attempt
    run_yt_dlp_stdout_stream(&bin_path, video_id, None, generation_pair, cancel_flag).await
}

async fn run_yt_dlp_stdout_stream(
    bin_path: &PathBuf,
    video_id: &str,
    cookies_file: Option<PathBuf>,
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<ExtractedAudio, String> {
    let mut cookie_guard = CookieFileGuard(cookies_file);
    let has_cookies = cookie_guard.as_ref().is_some();
    let cookie_path_for_redaction = cookie_guard.as_ref().cloned();

    let effective_cancel = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let cancel_opt = Some(effective_cancel.clone());

    let url = format!("https://www.youtube.com/watch?v={video_id}");

    let mut cmd = Command::new(bin_path);
    cmd.arg("--no-playlist")
        .arg("--no-config")
        .arg("--no-exec")
        .arg("-f")
        // Only select containers/codecs supported by rodio + Symphonia (M4A/AAC, MP4, MP3, OGG/Vorbis, FLAC, WAV)
        // so yt-dlp never falls back to WebM/Opus (`itag 251`) which Symphonia cannot demux.
        .arg("140/bestaudio[ext=m4a]/bestaudio[ext=mp4]/bestaudio[ext=mp3]/bestaudio[ext=ogg]/bestaudio[acodec^=mp4a]")
        .arg("-o")
        .arg("-")
        .arg("--quiet")
        .arg("--no-warnings")
        .arg("--no-part")
        .arg("--no-cache-dir");

    if let Some(cookie_path) = cookie_guard.as_ref() {
        cmd.arg("--cookies").arg(cookie_path);
    }

    cmd.arg("--")
        .arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW prevents flashing a console window per track on Windows
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Falha ao executar yt-dlp: {e}"))?;

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
        if is_stream_cancelled(&generation_pair, &cancel_opt, None) {
            let _ = child.kill().await;
            return Err(CANCELLED_ERR_MSG.to_string());
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
        drop(stdout);
        let stderr_fut = async {
            let mut err_bytes = Vec::new();
            if let Some(err_pipe) = stderr.take() {
                let _ = err_pipe
                    .take(MAX_YT_DLP_STDERR_BYTES)
                    .read_to_end(&mut err_bytes)
                    .await;
            }
            String::from_utf8_lossy(&err_bytes).into_owned()
        };
        let (status_res, raw_err_msg) = tokio::join!(child.wait(), stderr_fut);
        let exit_code = status_res.ok().and_then(|s| s.code());
        let clean_err = sanitize_yt_dlp_stderr(&raw_err_msg, cookie_path_for_redaction.as_ref());
        if !clean_err.is_empty() {
            return Err(format!("yt-dlp falhou para {video_id}: {clean_err}"));
        }
        // On Windows, exit code -1073741515 (0xC0000135 STATUS_DLL_NOT_FOUND) indicates
        // Microsoft Visual C++ Redistributable (vcruntime140.dll) is missing on the machine.
        if exit_code == Some(-1073741515) {
            return Err(
                "O yt-dlp.exe requer o Microsoft Visual C++ Redistributable (x64) instalado no Windows."
                    .to_string(),
            );
        }
        return Err(format!(
            "yt-dlp retornou buffer insuficiente ({} bytes, status: {:?}) para {video_id}",
            progressive_buffer.len(),
            exit_code
        ));
    }

    let probe_abort = Arc::new(AtomicBool::new(false));
    let probe_abort_notify = Arc::new(Notify::new());

    // Spawn the background chunk reader via async runtime without allocating dedicated OS thread stack.
    // Transfer sole ownership of `cookie_guard` to the background task when `!eof_reached`.
    if !eof_reached {
        drop(stderr);
        let bg_buffer = progressive_buffer.clone();
        let bg_gen = generation_pair.clone();
        let bg_cancel = cancel_opt.clone();
        let bg_probe_abort = probe_abort.clone();
        let bg_probe_notify = probe_abort_notify.clone();
        let bg_cookie_guard = CookieFileGuard(cookie_guard.take());
        tauri::async_runtime::spawn(async move {
            let _cookie_guard = bg_cookie_guard;
            let mut bg_chunk = [0u8; 32_768];
            loop {
                if is_stream_cancelled(&bg_gen, &bg_cancel, Some(&bg_probe_abort)) {
                    let _ = child.kill().await;
                    bg_buffer.mark_error(CANCELLED_ERR_MSG.to_string());
                    break;
                }

                let read_res = tokio::select! {
                    biased;
                    _ = bg_probe_notify.notified() => {
                        let _ = child.kill().await;
                        bg_buffer.mark_error("Probe de decodificação de áudio falhou".to_string());
                        break;
                    }
                    res = tokio::time::timeout(Duration::from_secs(30), stdout.read(&mut bg_chunk)) => res,
                };

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
                        bg_buffer.mark_error("Timeout no streaming progressivo".to_string());
                        break;
                    }
                }
            }
            let _ = child.wait().await;
            drop(_cookie_guard);
        });
    } else {
        drop(stdout);
        drop(stderr);
        let _ = child.wait().await;
        drop(cookie_guard);
    }

    // Always verify that `rodio::Decoder` can actually initialize on the stream before returning
    // `Ok(ExtractedAudio)`, and use the `sidx` duration when present.
    let sidx_meta = progressive_buffer.inspect_mp4_sidx();
    if let Some((exact_bytes, _)) = sidx_meta {
        progressive_buffer.set_total_expected(exact_bytes.min(max_bytes as u64));
    }

    let probe_buffer = progressive_buffer.clone();
    let vid_owned = video_id.to_string();
    let probe_res = tokio::task::spawn_blocking(move || {
        let decoder = Decoder::new(probe_buffer.reader()).map_err(|e| {
            format!("Falha ao decodificar container de áudio para {vid_owned}: {e}")
        })?;
        let dur = decoder
            .total_duration()
            .map(|d| d.as_secs_f64())
            .filter(|&d| d > 1.0)
            .unwrap_or(0.0);
        Ok::<f64, String>(dur)
    })
    .await;

    let probed_duration = match probe_res {
        Ok(Ok(dur)) => dur,
        Ok(Err(err_msg)) => {
            probe_abort.store(true, Ordering::SeqCst);
            probe_abort_notify.notify_waiters();
            return Err(err_msg);
        }
        Err(join_err) => {
            probe_abort.store(true, Ordering::SeqCst);
            probe_abort_notify.notify_waiters();
            return Err(format!("Erro na thread de probe de áudio: {join_err}"));
        }
    };

    if is_stream_cancelled(&generation_pair, &cancel_opt, None) {
        probe_abort.store(true, Ordering::SeqCst);
        probe_abort_notify.notify_waiters();
        return Err(CANCELLED_ERR_MSG.to_string());
    }

    let decoded_duration = sidx_meta
        .map(|(_, sidx_dur)| sidx_dur)
        .filter(|&d| d > 1.0)
        .unwrap_or(probed_duration);

    let quality_label = if has_cookies {
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
        cancel_flag: Some(effective_cancel),
    })
}

/// Validates that a resolved YouTube media stream URL uses HTTPS and an allowed Google/YouTube host.
pub fn is_valid_stream_url(stream_url: &str) -> bool {
    let Ok(parsed_url) = reqwest::Url::parse(stream_url) else {
        return false;
    };
    if parsed_url.scheme() != "https" {
        return false;
    }
    let host = parsed_url.host_str().unwrap_or("");
    host.ends_with(".googlevideo.com") || host.ends_with(".youtube.com")
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
    generation_pair: Option<(u64, Arc<AtomicU64>)>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> Result<ExtractedAudio, String> {
    let client = crate::innertube::get_shared_http_client();

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
        if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
            return Err(CANCELLED_ERR_MSG.to_string());
        }

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

        let Ok(json_data) = crate::innertube::read_json_bounded(resp, 5 * 1024 * 1024).await else {
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
            // Skip `audio/webm` (Opus/Matroska) because Symphonia does not include an MKV/WebM demuxer
            if mime.starts_with("audio/webm") {
                continue;
            }
            let itag = fmt.get("itag").and_then(|i| i.as_i64()).unwrap_or(0);

            let score = if itag == 140 {
                100
            } else if mime.starts_with("audio/mp4") {
                80
            } else if mime.starts_with("audio/ogg") || mime.starts_with("audio/mpeg") {
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

        if !is_valid_stream_url(&stream_url) {
            continue;
        }

        if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
            return Err(CANCELLED_ERR_MSG.to_string());
        }

        let max_bytes: usize = 25 * 1024 * 1024;
        let get_req = client
            .get(&stream_url)
            .header("User-Agent", profile.user_agent)
            .header("Origin", profile.origin)
            .header("Range", format!("bytes=0-{}", max_bytes - 1));

        let Ok(mut audio_resp) = get_req.send().await else {
            continue;
        };

        if !audio_resp.status().is_success() && audio_resp.status().as_u16() != 206 {
            continue;
        }

        let initial_cap = audio_resp
            .content_length()
            .map(|n| (n as usize).min(max_bytes))
            .unwrap_or(1024 * 1024);
        let mut vec_data = Vec::with_capacity(initial_cap);
        let mut read_ok = true;

        loop {
            if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
                return Err(CANCELLED_ERR_MSG.to_string());
            }
            match audio_resp.chunk().await {
                Ok(Some(chunk)) => {
                    let remaining = max_bytes.saturating_sub(vec_data.len());
                    if remaining == 0 {
                        break;
                    }
                    if chunk.len() >= remaining {
                        vec_data.extend_from_slice(&chunk[..remaining]);
                        break;
                    }
                    vec_data.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(_) => {
                    read_ok = false;
                    break;
                }
            }
        }

        if read_ok && vec_data.len() > 16_384 {
            vec_data.shrink_to_fit();
            let prog_buf = ProgressiveAudioBuffer::from_vec(vec_data);
            let probe_buf = prog_buf.clone();
            let probe_ok =
                tokio::task::spawn_blocking(move || Decoder::new(probe_buf.reader()).is_ok())
                    .await
                    .unwrap_or(false);
            if probe_ok {
                if is_stream_cancelled(&generation_pair, &cancel_flag, None) {
                    return Err(CANCELLED_ERR_MSG.to_string());
                }
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
                    cancel_flag,
                });
            }
        }
    }

    Err("Não foi possível extrair o stream de áudio real do YouTube".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires live YouTube network access"]
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

        // Decode on a blocking thread while background tasks pump stdout
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

    fn build_mp4_box(box_type: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let total_len = (8 + payload.len()) as u32;
        let mut out = Vec::with_capacity(total_len as usize);
        out.extend_from_slice(&total_len.to_be_bytes());
        out.extend_from_slice(box_type);
        out.extend_from_slice(payload);
        out
    }

    fn build_sidx_v0(
        timescale: u32,
        earliest_pts: u32,
        first_offset: u32,
        refs: &[(u32, u32)],
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0u8, 0, 0, 0]); // version=0, flags=0
        payload.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
        payload.extend_from_slice(&timescale.to_be_bytes());
        payload.extend_from_slice(&earliest_pts.to_be_bytes());
        payload.extend_from_slice(&first_offset.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes()); // reserved
        payload.extend_from_slice(&(refs.len() as u16).to_be_bytes());
        for &(raw_size, sub_dur) in refs {
            payload.extend_from_slice(&raw_size.to_be_bytes());
            payload.extend_from_slice(&sub_dur.to_be_bytes());
            payload.extend_from_slice(&0u32.to_be_bytes()); // SAP info
        }
        build_mp4_box(b"sidx", &payload)
    }

    fn build_sidx_v1(
        timescale: u32,
        earliest_pts: u64,
        first_offset: u64,
        refs: &[(u32, u32)],
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[1u8, 0, 0, 0]); // version=1, flags=0
        payload.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
        payload.extend_from_slice(&timescale.to_be_bytes());
        payload.extend_from_slice(&earliest_pts.to_be_bytes());
        payload.extend_from_slice(&first_offset.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes()); // reserved
        payload.extend_from_slice(&(refs.len() as u16).to_be_bytes());
        for &(raw_size, sub_dur) in refs {
            payload.extend_from_slice(&raw_size.to_be_bytes());
            payload.extend_from_slice(&sub_dur.to_be_bytes());
            payload.extend_from_slice(&0u32.to_be_bytes()); // SAP info
        }
        build_mp4_box(b"sidx", &payload)
    }

    #[test]
    fn test_parse_mp4_sidx_metadata_valid_v0_and_v1() {
        let ftyp = build_mp4_box(b"ftyp", b"M4A \x00\x00\x00\x00M4A mp42");
        // Include MSB bit set (0x8000_0000) on first ref to ensure reference_type bit is masked out
        let sidx_v0 = build_sidx_v0(
            44_100,
            0,
            128,
            &[(0x8000_0000 | 1000, 44_100), (2500, 88_200)],
        );
        let mut stream = Vec::new();
        stream.extend_from_slice(&ftyp);
        stream.extend_from_slice(&sidx_v0);

        let expected_sidx_end = (ftyp.len() + sidx_v0.len()) as u64;
        let (total_bytes, duration_secs) =
            parse_mp4_sidx_metadata(&stream).expect("valid v0 sidx box should parse");
        assert_eq!(total_bytes, expected_sidx_end + 128 + 1000 + 2500);
        assert!((duration_secs - 3.0).abs() < 1e-6);

        // Test version 1 with 64-bit first_offset
        let sidx_v1 = build_sidx_v1(48_000, 0, 512, &[(4096, 96_000)]);
        let (v1_bytes, v1_dur) =
            parse_mp4_sidx_metadata(&sidx_v1).expect("valid v1 sidx box should parse");
        assert_eq!(v1_bytes, (sidx_v1.len() as u64) + 512 + 4096);
        assert!((v1_dur - 2.0).abs() < 1e-6);

        // Extreme u64::MAX first_offset saturates safely without integer overflow panic
        let sidx_huge = build_sidx_v1(48_000, 0, u64::MAX, &[(4096, 48_000)]);
        let (huge_bytes, _) =
            parse_mp4_sidx_metadata(&sidx_huge).expect("huge first_offset should saturate");
        assert_eq!(huge_bytes, u64::MAX);

        // Timescale == 0 should safely return 0.0 duration without NaN/Inf
        let zero_ts = build_sidx_v0(0, 0, 0, &[(1024, 1000)]);
        let (_, dur_zero) = parse_mp4_sidx_metadata(&zero_ts).unwrap();
        assert_eq!(dur_zero, 0.0);
    }

    #[test]
    fn test_parse_mp4_sidx_metadata_invalid_and_truncated() {
        assert_eq!(parse_mp4_sidx_metadata(&[]), None);
        assert_eq!(parse_mp4_sidx_metadata(b"short"), None);

        // Box size < 8 (invalid size=0 or size=4)
        let mut zero_size = vec![0, 0, 0, 0];
        zero_size.extend_from_slice(b"ftyp");
        assert_eq!(parse_mp4_sidx_metadata(&zero_size), None);

        // Only ftyp box, no sidx
        let ftyp_only = build_mp4_box(b"ftyp", b"M4A \x00\x00\x00\x00");
        assert_eq!(parse_mp4_sidx_metadata(&ftyp_only), None);

        // Valid sidx truncated mid-buffer
        let valid_sidx = build_sidx_v0(44_100, 0, 0, &[(1000, 44_100)]);
        assert_eq!(
            parse_mp4_sidx_metadata(&valid_sidx[..valid_sidx.len() - 4]),
            None
        );

        // Declared size < 32 for sidx
        let tiny_sidx = build_mp4_box(b"sidx", &[0u8; 16]);
        assert_eq!(parse_mp4_sidx_metadata(&tiny_sidx), None);

        // Version 1 box smaller than minimum 40 bytes
        let mut short_v1 = vec![1u8, 0, 0, 0];
        short_v1.extend_from_slice(&[0u8; 20]); // total box size = 32 < 40
        let short_v1_box = build_mp4_box(b"sidx", &short_v1);
        assert_eq!(parse_mp4_sidx_metadata(&short_v1_box), None);

        // Reference count larger than payload
        let mut bad_ref_count = build_sidx_v0(44_100, 0, 0, &[(100, 100)]);
        // Overwrite ref_count at offset 30..32 (box header 8 + 22 = 30) with 99
        bad_ref_count[30] = 0;
        bad_ref_count[31] = 99;
        assert_eq!(parse_mp4_sidx_metadata(&bad_ref_count), None);
    }

    #[test]
    fn test_progressive_audio_buffer_empty_finished_and_error_states() {
        let empty_done = ProgressiveAudioBuffer::new(64, None);
        empty_done.mark_finished();
        assert!(empty_done.is_empty());
        assert!(empty_done.is_finished());
        let mut r = empty_done.reader();
        let mut buf = [0u8; 16];
        assert_eq!(r.read(&mut buf).unwrap(), 0);

        // Empty read slice should immediately return Ok(0)
        assert_eq!(r.read(&mut []).unwrap(), 0);

        // Error on empty buffer propagates io::Error
        let err_buf = ProgressiveAudioBuffer::new(64, None);
        err_buf.mark_error("stream aborted".to_string());
        assert!(err_buf.is_finished());
        let err = err_buf.reader().read(&mut buf).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert!(err.to_string().contains("stream aborted"));

        // Non-empty buffer with mark_error still drains buffered bytes before EOF
        let partial_err = ProgressiveAudioBuffer::new(64, None);
        partial_err.push_chunk(&[]); // empty chunk is a no-op
        partial_err.push_chunk(b"ABC");
        partial_err.mark_error("late failure".to_string());
        let mut out = Vec::new();
        partial_err.reader().read_to_end(&mut out).unwrap();
        assert_eq!(out, b"ABC");
    }

    #[test]
    fn test_progressive_audio_buffer_seek_and_sidx_end_offset() {
        let buf = ProgressiveAudioBuffer::from_vec(b"0123456789".to_vec());
        assert_eq!(buf.current_bytes_len(), 10);
        assert_eq!(buf, ProgressiveAudioBuffer::from(b"0123456789".to_vec()));

        let mut reader = buf.reader();
        assert_eq!(reader.seek(SeekFrom::Start(4)).unwrap(), 4);
        let mut two = [0u8; 2];
        reader.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"45");

        assert_eq!(reader.seek(SeekFrom::Current(-3)).unwrap(), 3);
        reader.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"34");

        assert_eq!(reader.seek(SeekFrom::End(-2)).unwrap(), 8);
        reader.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"89");

        // Seeking past EOF when finished clamps to len()
        assert_eq!(reader.seek(SeekFrom::Start(999)).unwrap(), 10);
        assert_eq!(reader.read(&mut two).unwrap(), 0);

        // Seeking before byte 0 returns InvalidInput
        let neg_err = reader.seek(SeekFrom::Current(-100)).unwrap_err();
        assert_eq!(neg_err.kind(), io::ErrorKind::InvalidInput);
        let neg_end_err = reader.seek(SeekFrom::End(-50)).unwrap_err();
        assert_eq!(neg_end_err.kind(), io::ErrorKind::InvalidInput);

        // In-flight buffer containing valid sidx box resolves SeekFrom::End(0) immediately
        let sidx = build_sidx_v0(44_100, 0, 100, &[(500, 44_100)]);
        let expected_total = (sidx.len() as u64) + 100 + 500;
        let inflight = ProgressiveAudioBuffer::new(1024, None);
        inflight.push_chunk(&sidx);
        assert_eq!(inflight.inspect_mp4_sidx(), Some((expected_total, 1.0)));
        let mut inflight_reader = inflight.reader();
        assert_eq!(
            inflight_reader.seek(SeekFrom::End(0)).unwrap(),
            expected_total
        );

        // In-flight buffer WITHOUT sidx box returns UNKNOWN_PROGRESSIVE_END_SENTINEL immediately
        // on SeekFrom::End(0) without blocking for 20s!
        let non_dash_inflight = ProgressiveAudioBuffer::new(1024, None);
        non_dash_inflight.push_chunk(b"ID3-or-OggS-header-bytes");
        let mut non_dash_reader = non_dash_inflight.reader();
        let start_instant = std::time::Instant::now();
        assert_eq!(
            non_dash_reader.seek(SeekFrom::End(0)).unwrap(),
            UNKNOWN_PROGRESSIVE_END_SENTINEL
        );
        assert!(
            start_instant.elapsed() < Duration::from_millis(200),
            "SeekFrom::End(0) on in-flight non-DASH stream must not block!"
        );
    }

    #[test]
    fn test_is_valid_stream_url_and_parse_youtube_video_id() {
        assert!(is_valid_stream_url(
            "https://rr1---sn-4g5e6nsz.googlevideo.com/videoplayback?itag=140"
        ));
        assert!(is_valid_stream_url(
            "https://music.youtube.com/api/stream/123"
        ));
        assert!(!is_valid_stream_url(
            "http://rr1---sn-4g5e6nsz.googlevideo.com/videoplayback"
        ));
        assert!(!is_valid_stream_url(
            "https://googlevideo.com.evil.example/videoplayback"
        ));
        assert!(!is_valid_stream_url(
            "https://youtube.com.attacker.net/watch"
        ));
        assert!(!is_valid_stream_url("not a valid url"));
        assert!(!is_valid_stream_url("ftp://r1.googlevideo.com/a"));

        assert_eq!(
            parse_youtube_video_id("https://music.youtube.com/watch?v=MV_3Dpw-BRY&list=RDAMVM"),
            "MV_3Dpw-BRY"
        );
        assert_eq!(
            parse_youtube_video_id("https://youtu.be/fJ9rUzIMcZQ?t=42"),
            "fJ9rUzIMcZQ"
        );
        assert_eq!(
            parse_youtube_video_id("https://www.youtube.com/embed/4NRXx6U8ABQ"),
            "4NRXx6U8ABQ"
        );
        assert_eq!(
            parse_youtube_video_id("https://www.youtube.com/shorts/FGBhQbmPwH8"),
            "FGBhQbmPwH8"
        );
        assert_eq!(parse_youtube_video_id("  dQw4w9WgXcQ  "), "dQw4w9WgXcQ");
        assert_eq!(parse_youtube_video_id("   "), "dQw4w9WgXcQ");
        assert_eq!(parse_youtube_video_id("id!@#with$%^junk"), "idwithjunk");
    }

    #[test]
    fn test_cross_platform_yt_dlp_paths_and_redirect_allowlist() {
        let url = get_yt_dlp_download_url();
        assert!(url.starts_with("https://github.com/yt-dlp/yt-dlp/releases/latest/download/"));
        let bin_name = get_yt_dlp_binary_name();
        let bin_path = get_yt_dlp_bin_path();
        assert_eq!(
            bin_path.file_name().and_then(|n| n.to_str()),
            Some(bin_name)
        );

        let gh =
            reqwest::Url::parse("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp")
                .unwrap();
        let gh_sums = reqwest::Url::parse(YT_DLP_SHA256SUMS_URL).unwrap();
        let gh_cdn = reqwest::Url::parse(
            "https://objects.githubusercontent.com/github-production-release-asset/123",
        )
        .unwrap();
        let gh_release_assets =
            reqwest::Url::parse("https://release-assets.githubusercontent.com/file").unwrap();
        let http_gh = reqwest::Url::parse("http://github.com/yt-dlp/yt-dlp").unwrap();
        let evil = reqwest::Url::parse("https://evil.example.com/yt-dlp").unwrap();

        assert!(is_trusted_yt_dlp_download_url(&gh));
        assert!(is_trusted_yt_dlp_download_url(&gh_sums));
        assert!(is_trusted_yt_dlp_download_url(&gh_cdn));
        assert!(is_trusted_yt_dlp_download_url(&gh_release_assets));
        assert!(!is_trusted_yt_dlp_download_url(&http_gh));
        assert!(!is_trusted_yt_dlp_download_url(&evil));
    }

    #[test]
    fn test_sha256_verification_and_stderr_sanitization() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let digest = sha256_hex(b"abc");
        assert_eq!(digest.len(), 64);
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let sums = format!(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  yt-dlp.exe\n\
             {}  *yt-dlp_linux\n\
             malformed_line_only_one_token\n",
            digest.to_uppercase()
        );
        assert_eq!(
            parse_sha256sums_for_asset(&sums, "yt-dlp_linux").as_deref(),
            Some(digest.as_str())
        );
        assert_eq!(
            parse_sha256sums_for_asset(&sums, "yt-dlp.exe").as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert_eq!(parse_sha256sums_for_asset(&sums, "yt-dlp_macos"), None);

        let fake_cookie = PathBuf::from("/tmp/secret/cookies.123.tmp.txt");
        let raw_err = format!(
            "ERROR: failed reading {} because {}",
            fake_cookie.display(),
            "é".repeat(400)
        );
        let cleaned = sanitize_yt_dlp_stderr(&raw_err, Some(&fake_cookie));
        assert!(!cleaned.contains("/tmp/secret/cookies.123.tmp.txt"));
        assert!(cleaned.contains("[cookies]"));
        assert!(cleaned.chars().count() <= 300);
    }
}
