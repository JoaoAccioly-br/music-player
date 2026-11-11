use crate::auth::{generate_sapisidhash, AuthSession, FIREFOX_LINUX_UA, YTM_ORIGIN};
use rodio::{Decoder, OutputStream, Sink, Source};
use serde_json::{json, Value};
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub const YT_DLP_LINUX_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";

/// Commands sent from Tauri IPC / System Tray threads to the dedicated native audio thread.
/// Because `rodio::OutputStream` contains raw OS audio device handles (ALSA/PipeWire) that are
/// `!Send` and `!Sync`, keeping `OutputStream` and `Sink` inside a dedicated OS thread and
/// communicating via an `mpsc::Sender<AudioCommand>` is the idiomatic, zero-overhead Rust pattern.
pub enum AudioCommand {
    LoadAndPlay { data: Vec<u8>, volume: f32 },
    SetPlaying(bool),
    SetVolume(f32),
    Seek(f64),
    Stop,
}

/// Thread-safe handle to the dedicated native audio thread.
pub struct AudioEngine {
    sender: Mutex<Sender<AudioCommand>>,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        let (tx, rx) = channel::<AudioCommand>();

        thread::Builder::new()
            .name("ytm-native-audio".to_string())
            .spawn(move || {
                // Attempt to open the default OS audio output device (ALSA / PulseAudio / PipeWire).
                // `_stream` MUST stay alive on this thread for `Sink` to output sound.
                let mut stream_opt = OutputStream::try_default().ok();
                let mut sink_opt: Option<Sink> = None;
                let mut current_buffer: Option<Vec<u8>> = None;
                let mut current_volume: f32 = 0.8;
                let mut is_paused: bool = false;

                while let Ok(cmd) = rx.recv() {
                    // Lazily retry opening the audio device if it wasn't available at startup.
                    if stream_opt.is_none() {
                        stream_opt = OutputStream::try_default().ok();
                    }

                    match cmd {
                        AudioCommand::LoadAndPlay { data, volume } => {
                            current_volume = volume.clamp(0.0, 1.0);
                            is_paused = false;

                            // Drop previous sink immediately to stop playback and free decoder memory.
                            if let Some(old_sink) = sink_opt.take() {
                                old_sink.stop();
                            }

                            current_buffer = Some(data.clone());

                            if let Some((_, ref stream_handle)) = stream_opt {
                                if let Ok(new_sink) = Sink::try_new(stream_handle) {
                                    new_sink.set_volume(current_volume);
                                    let cursor = Cursor::new(data);
                                    if let Ok(decoder) = Decoder::new(cursor) {
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
                            let target = Duration::from_secs_f64(position_secs.max(0.0));
                            let mut seek_succeeded = false;

                            if let Some(ref sink) = sink_opt {
                                if !sink.empty() && sink.try_seek(target).is_ok() {
                                    seek_succeeded = true;
                                }
                            }

                            // Fallback if the sink had already finished or decoder required re-seeking from RAM buffer
                            if !seek_succeeded {
                                if let (Some((_, ref stream_handle)), Some(ref bytes)) =
                                    (&stream_opt, &current_buffer)
                                {
                                    if let Some(old_sink) = sink_opt.take() {
                                        old_sink.stop();
                                    }
                                    if let Ok(new_sink) = Sink::try_new(stream_handle) {
                                        new_sink.set_volume(current_volume);
                                        let cursor = Cursor::new(bytes.clone());
                                        if let Ok(mut decoder) = Decoder::new(cursor) {
                                            if decoder.try_seek(target).is_ok() {
                                                new_sink.append(decoder);
                                            } else {
                                                let cursor_fallback = Cursor::new(bytes.clone());
                                                if let Ok(dec_skip) = Decoder::new(cursor_fallback)
                                                {
                                                    new_sink.append(dec_skip.skip_duration(target));
                                                }
                                            }
                                            if is_paused {
                                                new_sink.pause();
                                            } else {
                                                new_sink.play();
                                            }
                                            sink_opt = Some(new_sink);
                                        }
                                    }
                                }
                            }
                        }
                        AudioCommand::Stop => {
                            if let Some(old_sink) = sink_opt.take() {
                                old_sink.stop();
                            }
                            current_buffer = None;
                        }
                    }
                }
            })
            .expect("failed to spawn native audio thread");

        Self {
            sender: Mutex::new(tx),
        }
    }

    pub fn send(&self, cmd: AudioCommand) {
        if let Ok(guard) = self.sender.lock() {
            let _ = guard.send(cmd);
        }
    }
}

/// Result of extracting a real YouTube audio stream into RAM.
#[derive(Debug, Clone)]
pub struct ExtractedAudio {
    pub data: Vec<u8>,
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
/// and sets executable permissions (`0755`).
pub async fn ensure_yt_dlp_binary() -> Result<PathBuf, String> {
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
fn write_temp_netscape_cookies(session: &AuthSession) -> Option<PathBuf> {
    if !session.is_authenticated || session.cookie_header.trim().is_empty() {
        return None;
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let cookies_path = PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("com.ytmusic.rust")
        .join("cookies.tmp.txt");

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
///    `m4a` (`itag 140` / `bestaudio[ext=m4a]`) bytes directly to `stdout` into a RAM `Vec<u8>`.
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

    // Step A: Primary extraction via isolated yt-dlp binary streaming to RAM
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
                        let _ = fs::remove_file(&cookies_file);
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
    let mut data = Vec::with_capacity(4 * 1024 * 1024);
    let mut chunk = [0u8; 32_768];

    loop {
        if let Some((expected_gen, ref counter)) = generation_pair {
            if counter.load(Ordering::SeqCst) != expected_gen {
                let _ = child.kill().await;
                return Err("Download cancelado (nova música selecionada)".to_string());
            }
        }

        let read_res = tokio::time::timeout(Duration::from_secs(30), stdout.read(&mut chunk)).await;
        let n = match read_res {
            Ok(Ok(0)) => break,
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

        data.extend_from_slice(&chunk[..n]);
        if data.len() >= max_bytes {
            data.truncate(max_bytes);
            let _ = child.kill().await;
            break;
        }
    }

    let status = child.wait().await.ok();

    if data.len() < 16_384 {
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
            data.len(),
            status.and_then(|s| s.code())
        ));
    }

    // Verify Symphonia/Rodio can decode the audio stream and read exact duration if available
    let cursor = Cursor::new(data.clone());
    let decoder = Decoder::new(cursor).map_err(|e| {
        format!("Falha ao decodificar container de áudio extraído para {video_id}: {e}")
    })?;

    let decoded_duration = decoder
        .total_duration()
        .map(|d| d.as_secs_f64())
        .filter(|&d| d > 1.0)
        .unwrap_or(0.0);

    let quality_label = if cookies_file.is_some() {
        "128 kbps M4A (yt-dlp • Sessão Autenticada em RAM)".to_string()
    } else {
        "128 kbps M4A (yt-dlp • YouTube Stream Puro em RAM)".to_string()
    };

    Ok(ExtractedAudio {
        data,
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
                if Decoder::new(Cursor::new(vec_data.clone())).is_ok() {
                    let quality_label = if auth.is_some_and(|a| a.is_authenticated) {
                        format!("128 kbps M4A ({} • Autenticado na RAM)", profile.name)
                    } else {
                        format!("128 kbps M4A ({} • YouTube Stream RAM)", profile.name)
                    };
                    return Ok(ExtractedAudio {
                        data: vec_data,
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

        // Extract two distinct real YouTube songs
        let audio1 = extract_youtube_audio_stream("dQw4w9WgXcQ", None)
            .await
            .expect("First real YouTube track (dQw4w9WgXcQ) should extract successfully");
        let audio2 = extract_youtube_audio_stream("fJ9rUzIMcZQ", None)
            .await
            .expect("Second real YouTube track (fJ9rUzIMcZQ) should extract successfully");

        assert!(
            audio1.data.len() > 50_000,
            "Audio 1 bytes should be > 50KB, got {}",
            audio1.data.len()
        );
        assert!(
            audio2.data.len() > 50_000,
            "Audio 2 bytes should be > 50KB, got {}",
            audio2.data.len()
        );
        assert_ne!(
            audio1.data, audio2.data,
            "Two different YouTube video IDs must produce distinct audio buffers"
        );

        let dec1 = Decoder::new(Cursor::new(audio1.data))
            .expect("rodio::Decoder must decode real M4A stream 1");
        let dec2 = Decoder::new(Cursor::new(audio2.data))
            .expect("rodio::Decoder must decode real M4A stream 2");

        assert!(dec1.channels() > 0 && dec1.sample_rate() > 0);
        assert!(dec2.channels() > 0 && dec2.sample_rate() > 0);
    }
}
