use crate::auth::{generate_sapisidhash, AuthSession, FIREFOX_LINUX_UA, YTM_ORIGIN};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

const YTM_API_BASE: &str = "https://music.youtube.com/youtubei/v1";
const WEB_REMIX_CLIENT_NAME: &str = "WEB_REMIX";
const WEB_REMIX_CLIENT_VERSION: &str = "1.20250101.01.00";
const WEB_REMIX_CLIENT_ID: &str = "67";

static SHARED_HTTP: OnceLock<reqwest::Client> = OnceLock::new();

/// Returns a lazily initialized, shared HTTP client with connection pooling and HTTP/2 reuse.
pub fn get_shared_http_client() -> reqwest::Client {
    SHARED_HTTP
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .pool_max_idle_per_host(8)
                .tcp_nodelay(true)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("Falha ao inicializar pool HTTP compartilhado")
        })
        .clone()
}

/// Validates identifier tokens like videoId or playlistId to contain only safe alphanumeric and hyphen/underscore characters.
pub fn is_valid_identifier(id: &str, max_len: usize) -> bool {
    !id.is_empty()
        && id.len() <= max_len
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Clean, frontend-ready representation of a playable YouTube Music track.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrackItem {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_text: String,
    pub duration_secs: f64,
    pub thumbnail_url: String,
}

/// Clean, frontend-ready representation of a YouTube Music playlist or album shelf item.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlaylistItem {
    pub playlist_id: String,
    pub title: String,
    pub subtitle: String,
    pub track_count_text: String,
    pub thumbnail_url: String,
}

/// A horizontal shelf / section on the YouTube Music Home screen.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HomeSection {
    pub title: String,
    pub items: Vec<TrackItem>,
}

/// A single lyric line with an optional timestamp in seconds (from LRC synced lyrics).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedLyricLine {
    pub time_secs: Option<f64>,
    pub text: String,
}

/// Clean, frontend-ready representation of track lyrics fetched from LRCLIB (synced LRC) or YouTube Music Innertube.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricsResponse {
    pub video_id: String,
    pub has_lyrics: bool,
    pub is_synced: bool,
    pub lines: Vec<String>,
    pub timed_lines: Vec<TimedLyricLine>,
    pub source_label: String,
}

/// Native Rust client for YouTube Music's internal Innertube (`WEB_REMIX`) API.
pub struct InnertubeClient {
    http: reqwest::Client,
}

impl InnertubeClient {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            http: get_shared_http_client(),
        })
    }

    fn base_context() -> Value {
        json!({
            "client": {
                "clientName": WEB_REMIX_CLIENT_NAME,
                "clientVersion": WEB_REMIX_CLIENT_VERSION,
                "hl": "pt",
                "gl": "BR",
                "platform": "DESKTOP",
                "osName": "Linux",
                "osVersion": "x86_64"
            },
            "user": {
                "lockedSafetyMode": false
            }
        })
    }

    pub async fn post_endpoint(
        &self,
        endpoint: &str,
        mut payload: Value,
        auth: Option<&AuthSession>,
    ) -> Result<Value, String> {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("context".to_string(), Self::base_context());
        }

        let url = format!("{YTM_API_BASE}/{endpoint}?prettyPrint=false");
        let mut req = self
            .http
            .post(&url)
            .header("User-Agent", FIREFOX_LINUX_UA)
            .header("Content-Type", "application/json")
            .header("Origin", YTM_ORIGIN)
            .header("Referer", "https://music.youtube.com/")
            .header("X-Origin", YTM_ORIGIN)
            .header("X-YouTube-Client-Name", WEB_REMIX_CLIENT_ID)
            .header("X-YouTube-Client-Version", WEB_REMIX_CLIENT_VERSION)
            .header("X-Goog-Visitor-Id", "");

        if let Some(session) = auth {
            if session.is_authenticated && !session.sapisid.is_empty() {
                let sapisidhash = generate_sapisidhash(&session.sapisid, YTM_ORIGIN);
                req = req
                    .header("Cookie", &session.cookie_header)
                    .header("Authorization", sapisidhash)
                    .header("X-Goog-AuthUser", "0");
            }
        }

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Erro de rede na chamada Innertube ({endpoint}): {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Innertube ({endpoint}) retornou HTTP {}",
                resp.status()
            ));
        }

        resp.json::<Value>()
            .await
            .map_err(|e| format!("Falha ao decodificar JSON do Innertube ({endpoint}): {e}"))
    }
}

/// Fetches the YouTube Music Home feed (`FEmusic_home`), returning sections of playable tracks.
/// Automatically falls back to curated real YouTube Music shelves if unauthenticated
/// or if Innertube returns empty shelves.
pub async fn fetch_home(auth: Option<&AuthSession>) -> Result<Vec<HomeSection>, String> {
    let client = InnertubeClient::new()?;
    let payload = json!({
        "browseId": "FEmusic_home"
    });

    if let Ok(data) = client.post_endpoint("browse", payload, auth).await {
        let sections = extract_home_sections(&data);
        if !sections.is_empty() {
            return Ok(sections);
        }
    }

    Ok(curated_fallback_home_sections())
}

/// Maps a user-facing search filter key to the official YouTube Music `WEB_REMIX` `params` token.
pub fn resolve_search_filter_params(filter: Option<&str>) -> Option<&'static str> {
    let f = filter?.trim().to_lowercase();
    match f.as_str() {
        "" | "all" | "tudo" => None,
        "songs" | "musicas" | "músicas" => Some("EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D"),
        "videos" | "vídeos" => Some("EgWKAQIQAWoKEAkQChAFEAMQBA%3D%3D"),
        "albums" | "albuns" | "álbuns" => Some("EgWKAQIYAWoKEAkQChAFEAMQBA%3D%3D"),
        "artists" | "artistas" => Some("EgWKAQIgAWoKEAkQChAFEAMQBA%3D%3D"),
        "playlists" => Some("EgeKAQQoADgBagwQDhAKEAMQBRAJEAQ%3D"),
        _ => None,
    }
}

/// Searches YouTube Music (`youtubei/v1/search`) for songs and videos matching `query`.
pub async fn search_music(
    query: &str,
    auth: Option<&AuthSession>,
) -> Result<Vec<TrackItem>, String> {
    search_music_filtered(query, None, auth).await
}

/// Searches YouTube Music (`youtubei/v1/search`) with optional `WEB_REMIX` category filter `params`.
/// Automatically falls back to an unfiltered search if the filtered query returns empty results.
pub async fn search_music_filtered(
    query: &str,
    filter: Option<&str>,
    auth: Option<&AuthSession>,
) -> Result<Vec<TrackItem>, String> {
    let mut trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.len() > 200 {
        trimmed = &trimmed[..200];
    }

    let client = InnertubeClient::new()?;

    // 1. If a specific filter parameter is requested, try filtered search first
    if let Some(params) = resolve_search_filter_params(filter) {
        let filtered_payload = json!({
            "query": trimmed,
            "params": params
        });

        if let Ok(data) = client.post_endpoint("search", filtered_payload, auth).await {
            let mut tracks = Vec::new();
            collect_tracks_recursive(&data, &mut tracks);
            deduplicate_tracks(&mut tracks);
            if !tracks.is_empty() {
                return Ok(tracks);
            }
        }
    }

    // 2. Unfiltered search (or automatic fallback if filtered search returned empty)
    let payload = json!({
        "query": trimmed
    });

    if let Ok(data) = client.post_endpoint("search", payload, auth).await {
        let mut tracks = Vec::new();
        collect_tracks_recursive(&data, &mut tracks);
        deduplicate_tracks(&mut tracks);
        if !tracks.is_empty() {
            return Ok(tracks);
        }
    }

    // 3. Resilient fallback search over curated real YouTube Music catalog if offline/restricted
    let lower = trimmed.to_lowercase();
    let mut fallback: Vec<TrackItem> = curated_fallback_home_sections()
        .into_iter()
        .flat_map(|s| s.items)
        .filter(|t| {
            t.title.to_lowercase().contains(&lower)
                || t.artist.to_lowercase().contains(&lower)
                || t.album.to_lowercase().contains(&lower)
        })
        .collect();

    if fallback.is_empty() {
        fallback = curated_fallback_home_sections()
            .into_iter()
            .next()
            .map(|s| s.items)
            .unwrap_or_default();
    }

    Ok(fallback)
}

/// Fetches the user's "Liked Music" playlist (`browseId: "VLLM"`).
pub async fn fetch_liked_music(auth: Option<&AuthSession>) -> Result<Vec<TrackItem>, String> {
    let client = InnertubeClient::new()?;
    let payload = json!({
        "browseId": "VLLM"
    });

    if let Ok(data) = client.post_endpoint("browse", payload, auth).await {
        let mut tracks = Vec::new();
        collect_tracks_recursive(&data, &mut tracks);
        deduplicate_tracks(&mut tracks);
        if !tracks.is_empty() {
            return Ok(tracks);
        }
    }

    // Provide curated real YouTube Music tracks when unauthenticated so playback works immediately
    Ok(curated_fallback_liked_tracks())
}

/// Fetches the user's saved playlists from their library (`browseId: "FEmusic_liked_playlists"`).
pub async fn fetch_user_playlists(auth: Option<&AuthSession>) -> Result<Vec<PlaylistItem>, String> {
    let client = InnertubeClient::new()?;
    let payload = json!({
        "browseId": "FEmusic_liked_playlists"
    });

    if let Ok(data) = client.post_endpoint("browse", payload, auth).await {
        let mut playlists = Vec::new();
        collect_playlists_recursive(&data, &mut playlists);
        deduplicate_playlists(&mut playlists);
        if !playlists.is_empty() {
            return Ok(playlists);
        }
    }

    Ok(curated_fallback_playlists())
}

/// Fetches tracks inside a specific playlist (`browseId` prefixed with `VL` if needed).
pub async fn fetch_playlist_tracks(
    playlist_id: &str,
    auth: Option<&AuthSession>,
) -> Result<Vec<TrackItem>, String> {
    let trimmed = playlist_id.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let bare_id = trimmed
        .strip_prefix("VL")
        .or_else(|| trimmed.strip_prefix("FE"))
        .unwrap_or(trimmed);
    if !is_valid_identifier(bare_id, 64) {
        return Ok(curated_fallback_liked_tracks());
    }

    let browse_id = if trimmed.starts_with("VL") || trimmed.starts_with("FE") {
        trimmed.to_string()
    } else {
        format!("VL{trimmed}")
    };

    let client = InnertubeClient::new()?;
    let payload = json!({
        "browseId": browse_id
    });

    if let Ok(data) = client.post_endpoint("browse", payload, auth).await {
        let mut tracks = Vec::new();
        collect_tracks_recursive(&data, &mut tracks);
        deduplicate_tracks(&mut tracks);
        if !tracks.is_empty() {
            return Ok(tracks);
        }
    }

    Ok(curated_fallback_liked_tracks())
}

/// Phase 5: Fetches continuous Radio / Autoplay recommendations (`/youtubei/v1/next`)
/// based on the currently finishing `video_id`, ensuring uninterrupted music during gaming sessions.
pub async fn fetch_radio_up_next(
    video_id: &str,
    auth: Option<&AuthSession>,
) -> Result<Vec<TrackItem>, String> {
    let trimmed_id = video_id.trim();
    if !is_valid_identifier(trimmed_id, 32) {
        return Ok(curated_fallback_liked_tracks());
    }

    let client = InnertubeClient::new()?;
    let payload = json!({
        "videoId": trimmed_id,
        "isAudioOnly": true,
        "enablePersistentPlaylistPanel": true
    });

    if let Ok(data) = client.post_endpoint("next", payload, auth).await {
        let mut tracks = Vec::new();
        collect_tracks_recursive(&data, &mut tracks);
        deduplicate_tracks(&mut tracks);
        tracks.retain(|t| t.video_id != trimmed_id);
        if !tracks.is_empty() {
            return Ok(tracks);
        }
    }

    // Resilient fallback Radio queue excluding the current track so Autoplay never stops during a game
    let mut fallback: Vec<TrackItem> = curated_fallback_liked_tracks()
        .into_iter()
        .filter(|t| t.video_id != trimmed_id)
        .collect();

    if fallback.is_empty() {
        fallback = curated_fallback_liked_tracks();
    }

    Ok(fallback)
}

/// Fetches track lyrics, prioritizing timestamp-synchronized LRC lyrics from LRCLIB
/// and falling back to YouTube Music Innertube (`/youtubei/v1/next` -> `MPLYt...` -> `/youtubei/v1/browse`)
/// or plain LRCLIB lyrics when synced lyrics are unavailable.
pub async fn fetch_track_lyrics(
    video_id: &str,
    title: Option<&str>,
    artist: Option<&str>,
    auth: Option<&AuthSession>,
) -> Result<LyricsResponse, String> {
    let trimmed_id = video_id.trim();
    if !trimmed_id.is_empty() && !is_valid_identifier(trimmed_id, 32) {
        return Ok(LyricsResponse {
            video_id: trimmed_id.to_string(),
            has_lyrics: false,
            is_synced: false,
            lines: Vec::new(),
            timed_lines: Vec::new(),
            source_label: "Identificador de faixa inválido".to_string(),
        });
    }

    let client = InnertubeClient::new()?;
    let clean_title = title.map(|s| s.trim()).unwrap_or("");
    let clean_artist = artist.map(|s| s.trim()).unwrap_or("");

    // 1. Prioritize LRCLIB for timestamp-synchronized LRC lyrics
    let mut plain_lrclib_fallback: Option<Vec<TimedLyricLine>> = None;
    if !clean_title.is_empty() {
        if let Some((timed_lines, is_synced)) =
            fetch_lrclib_lyrics(&client.http, clean_title, clean_artist).await
        {
            if is_synced && !timed_lines.is_empty() {
                let lines = timed_lines.iter().map(|l| l.text.clone()).collect();
                return Ok(LyricsResponse {
                    video_id: trimmed_id.to_string(),
                    has_lyrics: true,
                    is_synced: true,
                    lines,
                    timed_lines,
                    source_label: "Sincronizada (LRCLIB)".to_string(),
                });
            } else if !timed_lines.is_empty() {
                plain_lrclib_fallback = Some(timed_lines);
            }
        }
    }

    // 2. Try YouTube Music Innertube `/youtubei/v1/next` -> `MPLYt...` -> `/youtubei/v1/browse`
    if !trimmed_id.is_empty() {
        let next_payload = json!({
            "videoId": trimmed_id
        });

        if let Ok(next_data) = client.post_endpoint("next", next_payload, auth).await {
            if let Some(lyrics_browse_id) = find_lyrics_browse_id_recursive(&next_data) {
                let browse_payload = json!({
                    "browseId": lyrics_browse_id
                });

                if let Ok(browse_data) = client.post_endpoint("browse", browse_payload, auth).await
                {
                    let lines = extract_innertube_lyrics_lines(&browse_data);
                    if !lines.is_empty() {
                        let timed_lines = lines
                            .iter()
                            .map(|text| TimedLyricLine {
                                time_secs: None,
                                text: text.clone(),
                            })
                            .collect();
                        return Ok(LyricsResponse {
                            video_id: trimmed_id.to_string(),
                            has_lyrics: true,
                            is_synced: false,
                            lines,
                            timed_lines,
                            source_label: "YouTube Music (estimada)".to_string(),
                        });
                    }
                }
            }
        }
    }

    // 3. Fall back to plain LRCLIB lyrics if found
    if let Some(timed_lines) = plain_lrclib_fallback {
        let lines = timed_lines.iter().map(|l| l.text.clone()).collect();
        return Ok(LyricsResponse {
            video_id: trimmed_id.to_string(),
            has_lyrics: true,
            is_synced: false,
            lines,
            timed_lines,
            source_label: "LRCLIB (estimada)".to_string(),
        });
    }

    Ok(LyricsResponse {
        video_id: trimmed_id.to_string(),
        has_lyrics: false,
        is_synced: false,
        lines: Vec::new(),
        timed_lines: Vec::new(),
        source_label: "Letra indisponível para esta faixa".to_string(),
    })
}

fn find_lyrics_browse_id_recursive(node: &Value) -> Option<String> {
    match node {
        Value::Object(map) => {
            if let Some(tab) = map.get("tabRenderer") {
                let tab_title = tab
                    .get("title")
                    .and_then(|v| v.as_str())
                    .or_else(|| tab.pointer("/title/runs/0/text").and_then(|v| v.as_str()))
                    .unwrap_or("");

                let lower_title = tab_title.to_lowercase();
                if lower_title.contains("lyrics") || lower_title.contains("letra") {
                    if let Some(bid) = tab
                        .pointer("/endpoint/browseEndpoint/browseId")
                        .and_then(|v| v.as_str())
                    {
                        if !bid.trim().is_empty() {
                            return Some(bid.trim().to_string());
                        }
                    }
                }
            }

            if let Some(browse_ep) = map.get("browseEndpoint") {
                if let Some(bid) = browse_ep.get("browseId").and_then(|v| v.as_str()) {
                    if bid.trim().starts_with("MPLYt") {
                        return Some(bid.trim().to_string());
                    }
                }
            }

            for val in map.values() {
                if let Some(found) = find_lyrics_browse_id_recursive(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(found) = find_lyrics_browse_id_recursive(item) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_innertube_lyrics_lines(node: &Value) -> Vec<String> {
    let mut raw_chunks = Vec::new();
    collect_lyrics_description_recursive(node, &mut raw_chunks);

    if raw_chunks.is_empty() {
        return Vec::new();
    }

    let combined = raw_chunks.join("\n");
    combined
        .lines()
        .map(|l| l.trim().to_string())
        .collect::<Vec<String>>()
}

fn collect_lyrics_description_recursive(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            if let Some(shelf) = map.get("musicDescriptionShelfRenderer") {
                if let Some(runs) = shelf
                    .pointer("/description/runs")
                    .and_then(|v| v.as_array())
                {
                    let text: String = runs
                        .iter()
                        .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                        .collect();
                    if !text.trim().is_empty() {
                        out.push(text);
                    }
                }
                return;
            }
            for val in map.values() {
                collect_lyrics_description_recursive(val, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_lyrics_description_recursive(item, out);
            }
        }
        _ => {}
    }
}

fn sanitize_track_title_for_lyrics(title: &str) -> String {
    let mut cleaned = title.trim().to_string();
    for marker in [
        "(Official Video)",
        "(Official Music Video)",
        "(Official Audio)",
        "(Official Lyric Video)",
        "(Lyric Video)",
        "(Lyrics)",
        "(Audio)",
        "[Official Video]",
        "[Official Music Video]",
        "[Official Audio]",
        "[Lyrics]",
        "[Audio]",
        "[HD]",
        "[4K]",
    ] {
        if let Some(idx) = cleaned.to_lowercase().find(&marker.to_lowercase()) {
            cleaned.replace_range(idx..idx + marker.len(), "");
        }
    }
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn fetch_lrclib_lyrics(
    http: &reqwest::Client,
    title: &str,
    artist: &str,
) -> Option<(Vec<TimedLyricLine>, bool)> {
    let cleaned_title = sanitize_track_title_for_lyrics(title);
    let clean_artist = artist
        .split([',', '&'])
        .next()
        .unwrap_or(artist)
        .trim();
    let has_valid_artist = !clean_artist.is_empty()
        && !clean_artist.eq_ignore_ascii_case("YouTube Music")
        && !clean_artist.eq_ignore_ascii_case("YouTube Music Radio");

    let mut plain_candidate: Option<Vec<TimedLyricLine>> = None;

    // 1. Try direct `/api/get` when both artist and title are known
    if has_valid_artist {
        for candidate_title in [title, cleaned_title.as_str()] {
            if candidate_title.is_empty() {
                continue;
            }
            if let Ok(resp) = http
                .get("https://lrclib.net/api/get")
                .header("User-Agent", FIREFOX_LINUX_UA)
                .query(&[("artist_name", clean_artist), ("track_name", candidate_title)])
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(json_val) = resp.json::<Value>().await {
                        if let Some((lines, is_synced)) = parse_lrclib_record(&json_val) {
                            if is_synced {
                                return Some((lines, true));
                            }
                            if plain_candidate.is_none() {
                                plain_candidate = Some(lines);
                            }
                        }
                    }
                }
            }
            if candidate_title == cleaned_title {
                break;
            }
        }
    }

    // 2. Fallback to `/api/search?q=...`, scanning all hits for a synced LRC entry first
    let search_q = if has_valid_artist {
        format!("{clean_artist} {cleaned_title}")
    } else {
        cleaned_title.clone()
    };

    if let Ok(resp) = http
        .get("https://lrclib.net/api/search")
        .header("User-Agent", FIREFOX_LINUX_UA)
        .query(&[("q", search_q.as_str())])
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(Value::Array(items)) = resp.json::<Value>().await {
                for item in &items {
                    if let Some((lines, is_synced)) = parse_lrclib_record(item) {
                        if is_synced {
                            return Some((lines, true));
                        }
                        if plain_candidate.is_none() {
                            plain_candidate = Some(lines);
                        }
                    }
                }
            }
        }
    }

    plain_candidate.map(|lines| (lines, false))
}

fn parse_lrclib_record(record: &Value) -> Option<(Vec<TimedLyricLine>, bool)> {
    if let Some(synced) = record.get("syncedLyrics").and_then(|v| v.as_str()) {
        let timed = parse_lrc_lyrics(synced);
        if !timed.is_empty() {
            return Some((timed, true));
        }
    }

    if let Some(plain) = record.get("plainLyrics").and_then(|v| v.as_str()) {
        let lines: Vec<TimedLyricLine> = plain
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .map(|text| TimedLyricLine {
                time_secs: None,
                text,
            })
            .collect();
        if !lines.is_empty() {
            return Some((lines, false));
        }
    }

    None
}

fn parse_lrc_timestamp_tag(tag: &str) -> Option<f64> {
    let parts: Vec<&str> = tag.split(':').collect();
    match parts.len() {
        2 => {
            let mins: f64 = parts[0].trim().parse().ok()?;
            let secs: f64 = parts[1].trim().replace(',', ".").parse().ok()?;
            if mins >= 0.0 && secs >= 0.0 {
                Some(mins * 60.0 + secs)
            } else {
                None
            }
        }
        3 => {
            // Supports either hh:mm:ss or mm:ss:cc
            let p0: f64 = parts[0].trim().parse().ok()?;
            let p1: f64 = parts[1].trim().parse().ok()?;
            let p2: f64 = parts[2].trim().replace(',', ".").parse().ok()?;
            if parts[2].contains('.') || parts[2].contains(',') {
                Some(p0 * 3600.0 + p1 * 60.0 + p2)
            } else {
                Some(p0 * 60.0 + p1 + p2 / 100.0)
            }
        }
        _ => None,
    }
}

pub fn parse_lrc_lyrics(synced: &str) -> Vec<TimedLyricLine> {
    let mut offset_secs: f64 = 0.0;
    let mut timed_lines: Vec<TimedLyricLine> = Vec::new();

    for raw_line in synced.lines() {
        let mut rest = raw_line.trim();
        if rest.is_empty() {
            continue;
        }

        // Check for [offset:ms] tag
        if rest.starts_with("[offset:") && rest.ends_with(']') {
            if let Ok(ms) = rest[8..rest.len() - 1].trim().parse::<f64>() {
                offset_secs = ms / 1000.0;
            }
            continue;
        }

        let mut timestamps: Vec<f64> = Vec::new();
        while rest.starts_with('[') {
            let Some(close_idx) = rest.find(']') else {
                break;
            };
            let inside = rest[1..close_idx].trim();
            if let Some(ts) = parse_lrc_timestamp_tag(inside) {
                timestamps.push((ts - offset_secs).max(0.0));
                rest = rest[close_idx + 1..].trim_start();
            } else if inside.contains(':') {
                // Metadata tag like [ar:...], [ti:...], [al:...]
                rest = "";
                break;
            } else {
                break;
            }
        }

        if timestamps.is_empty() {
            continue;
        }

        let text = rest.trim();
        let display_text = if text.is_empty() {
            "♪".to_string()
        } else {
            text.to_string()
        };

        for ts in timestamps {
            timed_lines.push(TimedLyricLine {
                time_secs: Some(ts),
                text: display_text.clone(),
            });
        }
    }

    timed_lines.sort_by(|a, b| {
        a.time_secs
            .unwrap_or(0.0)
            .partial_cmp(&b.time_secs.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    while timed_lines.last().is_some_and(|l| l.text == "♪") {
        timed_lines.pop();
    }

    timed_lines
}

#[cfg(test)]
mod lyrics_tests {
    use super::*;

    #[test]
    fn test_parse_lrc_lyrics_with_timestamps_and_multi_tags() {
        let sample = "[ar:Kavinsky]
[ti:Nightcall]
[00:10.50] I'm giving you a night call
[00:15.20][01:20.00] To tell you how I feel
[00:22.00] 
[01:25.00] 
";
        let parsed = parse_lrc_lyrics(sample);
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].text, "I'm giving you a night call");
        assert!((parsed[0].time_secs.unwrap() - 10.5).abs() < 0.01);
        assert_eq!(parsed[1].text, "To tell you how I feel");
        assert!((parsed[1].time_secs.unwrap() - 15.2).abs() < 0.01);
        assert_eq!(parsed[2].text, "♪");
        assert!((parsed[2].time_secs.unwrap() - 22.0).abs() < 0.01);
        assert_eq!(parsed[3].text, "To tell you how I feel");
        assert!((parsed[3].time_secs.unwrap() - 80.0).abs() < 0.01);
    }
}

// ============================================================================
// Recursive Innertube JSON Walkers & Parsers
// ============================================================================

fn extract_home_sections(root: &Value) -> Vec<HomeSection> {
    let mut sections = Vec::new();
    collect_sections_recursive(root, &mut sections);

    if sections.is_empty() {
        let mut flat_tracks = Vec::new();
        collect_tracks_recursive(root, &mut flat_tracks);
        deduplicate_tracks(&mut flat_tracks);
        if !flat_tracks.is_empty() {
            sections.push(HomeSection {
                title: "Destaques do YouTube Music".to_string(),
                items: flat_tracks,
            });
        }
    }

    sections
}

fn collect_sections_recursive(node: &Value, out: &mut Vec<HomeSection>) {
    match node {
        Value::Object(map) => {
            for shelf_key in ["musicCarouselShelfRenderer", "musicShelfRenderer"] {
                if let Some(shelf) = map.get(shelf_key) {
                    let title =
                        extract_shelf_title(shelf).unwrap_or_else(|| "Recomendações".to_string());
                    let mut tracks = Vec::new();
                    if let Some(contents) = shelf.get("contents") {
                        collect_tracks_recursive(contents, &mut tracks);
                    }
                    deduplicate_tracks(&mut tracks);
                    if !tracks.is_empty() {
                        out.push(HomeSection {
                            title,
                            items: tracks,
                        });
                    }
                    return;
                }
            }

            for val in map.values() {
                collect_sections_recursive(val, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_sections_recursive(item, out);
            }
        }
        _ => {}
    }
}

fn extract_shelf_title(shelf: &Value) -> Option<String> {
    let candidates = [
        "/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text",
        "/title/runs/0/text",
        "/header/musicCardShelfHeaderBasicRenderer/title/runs/0/text",
    ];
    for ptr in candidates {
        if let Some(text) = shelf.pointer(ptr).and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn collect_tracks_recursive(node: &Value, out: &mut Vec<TrackItem>) {
    match node {
        Value::Object(map) => {
            if let Some(renderer) = map.get("playlistPanelVideoRenderer") {
                if let Some(track) = parse_playlist_panel_video_track(renderer) {
                    out.push(track);
                }
                return;
            }
            if let Some(renderer) = map.get("musicResponsiveListItemRenderer") {
                if let Some(track) = parse_responsive_list_item_track(renderer) {
                    out.push(track);
                }
                return;
            }
            if let Some(renderer) = map.get("musicTwoRowItemRenderer") {
                if let Some(track) = try_parse_two_row_track(renderer) {
                    out.push(track);
                }
                return;
            }
            for val in map.values() {
                collect_tracks_recursive(val, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_tracks_recursive(item, out);
            }
        }
        _ => {}
    }
}

fn parse_playlist_panel_video_track(renderer: &Value) -> Option<TrackItem> {
    let video_id = renderer
        .get("videoId")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .or_else(|| find_first_string_key(renderer, "videoId"))?;

    if video_id.is_empty() {
        return None;
    }

    let title = renderer
        .pointer("/title/runs/0/text")
        .and_then(|v| v.as_str())
        .or_else(|| {
            renderer
                .pointer("/title/simpleText")
                .and_then(|v| v.as_str())
        })
        .map(|s| s.trim().to_string())?;

    if title.is_empty() {
        return None;
    }

    let artist = renderer
        .pointer("/longBylineText/runs/0/text")
        .and_then(|v| v.as_str())
        .or_else(|| {
            renderer
                .pointer("/shortBylineText/runs/0/text")
                .and_then(|v| v.as_str())
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "YouTube Music Radio".to_string());

    let duration_text = renderer
        .pointer("/lengthText/runs/0/text")
        .and_then(|v| v.as_str())
        .or_else(|| {
            renderer
                .pointer("/lengthText/simpleText")
                .and_then(|v| v.as_str())
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "3:30".to_string());

    let duration_secs = parse_duration_secs(&duration_text).unwrap_or(210.0);

    let thumbnail_url = extract_best_thumbnail(renderer)
        .or_else(|| {
            renderer
                .pointer("/thumbnail/thumbnails/0/url")
                .and_then(|v| v.as_str())
                .filter(|u| u.starts_with("https://"))
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));

    Some(TrackItem {
        video_id,
        title,
        artist,
        album: "Rádio Automática".to_string(),
        duration_text,
        duration_secs,
        thumbnail_url,
    })
}

fn parse_responsive_list_item_track(renderer: &Value) -> Option<TrackItem> {
    let video_id = renderer
        .pointer("/playlistItemData/videoId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| find_first_string_key(renderer, "videoId"))?;

    if video_id.trim().is_empty() {
        return None;
    }

    let flex_cols = renderer.get("flexColumns").and_then(|v| v.as_array())?;
    let title = flex_cols
        .first()
        .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())?;

    if title.is_empty() {
        return None;
    }

    let mut artist = String::new();
    let mut album = String::new();
    let mut duration_text = String::new();

    // Parse metadata runs from secondary flex columns
    for col in flex_cols.iter().skip(1) {
        if let Some(runs) = col
            .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")
            .and_then(|v| v.as_array())
        {
            let tokens: Vec<String> = runs
                .iter()
                .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "•" && s != "-" && s != "&" && s != ",")
                .collect();

            for token in tokens {
                if looks_like_duration(&token) {
                    duration_text = token;
                } else if artist.is_empty()
                    && !token.eq_ignore_ascii_case("Música")
                    && !token.eq_ignore_ascii_case("Vídeo")
                    && !token.eq_ignore_ascii_case("Song")
                    && !token.eq_ignore_ascii_case("Video")
                {
                    artist = token;
                } else if album.is_empty() && token != artist {
                    album = token;
                }
            }
        }
    }

    // Check fixedColumns for explicit duration
    if duration_text.is_empty() {
        if let Some(fixed_text) = renderer
            .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text")
            .and_then(|v| v.as_str())
        {
            if looks_like_duration(fixed_text.trim()) {
                duration_text = fixed_text.trim().to_string();
            }
        }
    }

    if artist.is_empty() {
        artist = "YouTube Music".to_string();
    }
    if duration_text.is_empty() {
        duration_text = "3:30".to_string();
    }
    let duration_secs = parse_duration_secs(&duration_text).unwrap_or(210.0);

    let thumbnail_url = extract_best_thumbnail(renderer)
        .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));

    Some(TrackItem {
        video_id,
        title,
        artist,
        album,
        duration_text,
        duration_secs,
        thumbnail_url,
    })
}

fn try_parse_two_row_track(renderer: &Value) -> Option<TrackItem> {
    let video_id = renderer
        .pointer("/navigationEndpoint/watchEndpoint/videoId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| find_first_string_key(renderer, "videoId"))?;

    if video_id.trim().is_empty() {
        return None;
    }

    let title = renderer
        .pointer("/title/runs/0/text")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())?;

    if title.is_empty() {
        return None;
    }

    let mut artist = String::new();
    let mut duration_text = String::new();

    if let Some(runs) = renderer
        .pointer("/subtitle/runs")
        .and_then(|v| v.as_array())
    {
        for run in runs {
            if let Some(t) = run.get("text").and_then(|v| v.as_str()) {
                let clean = t.trim();
                if clean.is_empty() || clean == "•" || clean == "-" || clean == "&" || clean == ","
                {
                    continue;
                }
                if looks_like_duration(clean) {
                    duration_text = clean.to_string();
                } else if artist.is_empty()
                    && !clean.eq_ignore_ascii_case("Música")
                    && !clean.eq_ignore_ascii_case("Vídeo")
                    && !clean.eq_ignore_ascii_case("Song")
                    && !clean.eq_ignore_ascii_case("Video")
                {
                    artist = clean.to_string();
                }
            }
        }
    }

    // Also inspect thumbnailOverlay / thumbnailOverlays for a duration badge if subtitle didn't include one
    if duration_text.is_empty() {
        for overlay_key in ["thumbnailOverlay", "thumbnailOverlays"] {
            if let Some(overlay_node) = renderer.get(overlay_key) {
                if let Some(found_dur) = find_duration_token_recursive(overlay_node) {
                    duration_text = found_dur;
                    break;
                }
            }
        }
    }

    if artist.is_empty() {
        artist = "YouTube Music".to_string();
    }

    let (duration_text, duration_secs) = if !duration_text.is_empty() {
        let secs = parse_duration_secs(&duration_text).unwrap_or(210.0);
        (duration_text, secs)
    } else {
        ("3:30".to_string(), 210.0)
    };

    let thumbnail_url = extract_best_thumbnail(renderer)
        .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));

    Some(TrackItem {
        video_id,
        title,
        artist,
        album: String::new(),
        duration_text,
        duration_secs,
        thumbnail_url,
    })
}

fn find_duration_token_recursive(node: &Value) -> Option<String> {
    match node {
        Value::String(s) => {
            let trimmed = s.trim();
            if looks_like_duration(trimmed) {
                Some(trimmed.to_string())
            } else {
                None
            }
        }
        Value::Object(map) => {
            for val in map.values() {
                if let Some(found) = find_duration_token_recursive(val) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(found) = find_duration_token_recursive(item) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn collect_playlists_recursive(node: &Value, out: &mut Vec<PlaylistItem>) {
    match node {
        Value::Object(map) => {
            if let Some(renderer) = map.get("musicTwoRowItemRenderer") {
                if let Some(pl) = parse_two_row_playlist(renderer) {
                    out.push(pl);
                }
                return;
            }
            if let Some(renderer) = map.get("musicResponsiveListItemRenderer") {
                if let Some(pl) = parse_responsive_playlist(renderer) {
                    out.push(pl);
                }
                return;
            }
            for val in map.values() {
                collect_playlists_recursive(val, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_playlists_recursive(item, out);
            }
        }
        _ => {}
    }
}

fn parse_two_row_playlist(renderer: &Value) -> Option<PlaylistItem> {
    let browse_id = renderer
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(|v| v.as_str())
        .or_else(|| {
            renderer
                .pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseId")
                .and_then(|v| v.as_str())
        })?;

    if !browse_id.starts_with("VL")
        && !browse_id.starts_with("PL")
        && !browse_id.starts_with("RD")
        && !browse_id.starts_with("OLAK")
    {
        return None;
    }

    let title = renderer
        .pointer("/title/runs/0/text")
        .and_then(|v| v.as_str())?
        .trim()
        .to_string();

    let subtitle = renderer
        .pointer("/subtitle/runs")
        .and_then(|v| v.as_array())
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_else(|| "Playlist do YouTube Music".to_string());

    let thumbnail_url = extract_best_thumbnail(renderer).unwrap_or_default();

    Some(PlaylistItem {
        playlist_id: browse_id.to_string(),
        title,
        subtitle: subtitle.clone(),
        track_count_text: subtitle,
        thumbnail_url,
    })
}

fn parse_responsive_playlist(renderer: &Value) -> Option<PlaylistItem> {
    let browse_id = renderer
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(|v| v.as_str())?;

    if !browse_id.starts_with("VL") && !browse_id.starts_with("PL") {
        return None;
    }

    let title = renderer
        .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
        .and_then(|v| v.as_str())?
        .trim()
        .to_string();

    let subtitle = renderer
        .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
        .and_then(|v| v.as_array())
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_else(|| "Playlist do YouTube Music".to_string());

    let thumbnail_url = extract_best_thumbnail(renderer).unwrap_or_default();

    Some(PlaylistItem {
        playlist_id: browse_id.to_string(),
        title,
        subtitle: subtitle.clone(),
        track_count_text: subtitle,
        thumbnail_url,
    })
}

fn find_first_string_key(node: &Value, target_key: &str) -> Option<String> {
    match node {
        Value::Object(map) => {
            if let Some(val) = map.get(target_key).and_then(|v| v.as_str()) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            for val in map.values() {
                if let Some(found) = find_first_string_key(val, target_key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(found) = find_first_string_key(item, target_key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_best_thumbnail(node: &Value) -> Option<String> {
    let pointers = [
        "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnail/thumbnails",
    ];
    for ptr in pointers {
        if let Some(arr) = node.pointer(ptr).and_then(|v| v.as_array()) {
            if let Some(url) = arr
                .last()
                .and_then(|item| item.get("url"))
                .and_then(|u| u.as_str())
            {
                if url.starts_with("https://") {
                    return Some(url.to_string());
                }
            }
        }
    }
    None
}

pub fn looks_like_duration(text: &str) -> bool {
    parse_duration_secs(text).is_some()
}

pub fn parse_duration_secs(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut total_secs: f64 = 0.0;
    for part in parts {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let val: f64 = part.parse().ok()?;
        total_secs = total_secs * 60.0 + val;
    }
    if total_secs > 0.0 {
        Some(total_secs)
    } else {
        None
    }
}

fn deduplicate_tracks(tracks: &mut Vec<TrackItem>) {
    let mut seen = HashSet::new();
    tracks.retain(|t| seen.insert(t.video_id.clone()));
}

fn deduplicate_playlists(playlists: &mut Vec<PlaylistItem>) {
    let mut seen = HashSet::new();
    playlists.retain(|p| seen.insert(p.playlist_id.clone()));
}

// ============================================================================
// Curated Real YouTube Music Fallback Catalogs (100% VOD Songs for yt-dlp)
// ============================================================================

pub fn curated_fallback_home_sections() -> Vec<HomeSection> {
    vec![
        HomeSection {
            title: "Escolhas Rápidas para Jogos (Low-RAM Mix)".to_string(),
            items: vec![
                TrackItem {
                    video_id: "MV_3Dpw-BRY".to_string(),
                    title: "Nightcall".to_string(),
                    artist: "Kavinsky".to_string(),
                    album: "OutRun".to_string(),
                    duration_text: "4:18".to_string(),
                    duration_secs: 258.0,
                    thumbnail_url: "https://i.ytimg.com/vi/MV_3Dpw-BRY/hqdefault.jpg".to_string(),
                },
                TrackItem {
                    video_id: "-5KAN9_CzSA".to_string(),
                    title: "coffee".to_string(),
                    artist: "beabadoobee".to_string(),
                    album: "Lofi & Chill Sessions".to_string(),
                    duration_text: "2:06".to_string(),
                    duration_secs: 126.0,
                    thumbnail_url: "https://i.ytimg.com/vi/-5KAN9_CzSA/hqdefault.jpg".to_string(),
                },
                TrackItem {
                    video_id: "hHW1oY26kxQ".to_string(),
                    title: "Snowman".to_string(),
                    artist: "WYS".to_string(),
                    album: "1 Am. Study Session".to_string(),
                    duration_text: "3:15".to_string(),
                    duration_secs: 195.0,
                    thumbnail_url: "https://i.ytimg.com/vi/hHW1oY26kxQ/hqdefault.jpg".to_string(),
                },
                TrackItem {
                    video_id: "rUxyKA_-grg".to_string(),
                    title: "Resonance".to_string(),
                    artist: "HOME".to_string(),
                    album: "Odyssey".to_string(),
                    duration_text: "3:32".to_string(),
                    duration_secs: 212.0,
                    thumbnail_url: "https://i.ytimg.com/vi/rUxyKA_-grg/hqdefault.jpg".to_string(),
                },
            ],
        },
        HomeSection {
            title: "Clássicos & Eletrônica (YouTube Music)".to_string(),
            items: vec![
                TrackItem {
                    video_id: "fJ9rUzIMcZQ".to_string(),
                    title: "Bohemian Rhapsody".to_string(),
                    artist: "Queen".to_string(),
                    album: "A Night at the Opera".to_string(),
                    duration_text: "5:55".to_string(),
                    duration_secs: 355.0,
                    thumbnail_url: "https://i.ytimg.com/vi/fJ9rUzIMcZQ/hqdefault.jpg".to_string(),
                },
                TrackItem {
                    video_id: "4NRXx6U8ABQ".to_string(),
                    title: "Blinding Lights".to_string(),
                    artist: "The Weeknd".to_string(),
                    album: "After Hours".to_string(),
                    duration_text: "3:20".to_string(),
                    duration_secs: 200.0,
                    thumbnail_url: "https://i.ytimg.com/vi/4NRXx6U8ABQ/hqdefault.jpg".to_string(),
                },
                TrackItem {
                    video_id: "FGBhQbmPwH8".to_string(),
                    title: "One More Time".to_string(),
                    artist: "Daft Punk".to_string(),
                    album: "Discovery".to_string(),
                    duration_text: "5:20".to_string(),
                    duration_secs: 320.0,
                    thumbnail_url: "https://i.ytimg.com/vi/FGBhQbmPwH8/hqdefault.jpg".to_string(),
                },
                TrackItem {
                    video_id: "dQw4w9WgXcQ".to_string(),
                    title: "Never Gonna Give You Up".to_string(),
                    artist: "Rick Astley".to_string(),
                    album: "Whenever You Need Somebody".to_string(),
                    duration_text: "3:33".to_string(),
                    duration_secs: 213.0,
                    thumbnail_url: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_string(),
                },
            ],
        },
    ]
}

pub fn curated_fallback_liked_tracks() -> Vec<TrackItem> {
    curated_fallback_home_sections()
        .into_iter()
        .flat_map(|s| s.items)
        .collect()
}

pub fn curated_fallback_playlists() -> Vec<PlaylistItem> {
    vec![
        PlaylistItem {
            playlist_id: "VLLM".to_string(),
            title: "Músicas Curtidas".to_string(),
            subtitle: "Playlist automática • Sua conta YouTube Music".to_string(),
            track_count_text: "Playlist automática".to_string(),
            thumbnail_url:
                "https://www.gstatic.com/youtube/media/ytm/images/pbg/liked-music-@576.png"
                    .to_string(),
        },
        PlaylistItem {
            playlist_id: "VLPL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i".to_string(),
            title: "Top 100 Music Videos Global".to_string(),
            subtitle: "YouTube Music • Atualizada semanalmente".to_string(),
            track_count_text: "100 faixas".to_string(),
            thumbnail_url: "https://i.ytimg.com/vi/4NRXx6U8ABQ/hqdefault.jpg".to_string(),
        },
        PlaylistItem {
            playlist_id: "VLPL6NdkXsPL07KN01gH2vucrHCEyyNmVEx4".to_string(),
            title: "Lofi Hip Hop & Synthwave Gaming".to_string(),
            subtitle: "Zero-WebView Low-RAM Mix".to_string(),
            track_count_text: "50 faixas".to_string(),
            thumbnail_url: "https://i.ytimg.com/vi/MV_3Dpw-BRY/hqdefault.jpg".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_lrc_lyrics_offset_and_extended_timestamps() {
        let lrc = "[offset:2500]
[00:05.00] First line shifted to 2.5s
[00:01.00] Clamped to zero
[01:02:03.50] Hour format line
[02:10:25] Centisecond colon format
[00:10,75] Comma decimal separator
";
        let parsed = parse_lrc_lyrics(lrc);
        assert_eq!(parsed.len(), 5);

        // Sorted ascending by time_secs
        assert_eq!(parsed[0].text, "Clamped to zero");
        assert!((parsed[0].time_secs.unwrap() - 0.0).abs() < 1e-6);

        assert_eq!(parsed[1].text, "First line shifted to 2.5s");
        assert!((parsed[1].time_secs.unwrap() - 2.5).abs() < 1e-6);

        assert_eq!(parsed[2].text, "Comma decimal separator");
        assert!((parsed[2].time_secs.unwrap() - 8.25).abs() < 1e-6);

        assert_eq!(parsed[3].text, "Centisecond colon format");
        // (2*60 + 10 + 0.25) - 2.5 = 127.75
        assert!((parsed[3].time_secs.unwrap() - 127.75).abs() < 1e-6);

        assert_eq!(parsed[4].text, "Hour format line");
        // (3600 + 120 + 3.5) - 2.5 = 3721.0
        assert!((parsed[4].time_secs.unwrap() - 3721.0).abs() < 1e-6);

        // Negative offset shifts timestamps later
        let neg_offset = "[offset:-1500]
[00:10.00] Delayed line";
        let parsed_neg = parse_lrc_lyrics(neg_offset);
        assert_eq!(parsed_neg.len(), 1);
        assert!((parsed_neg[0].time_secs.unwrap() - 11.5).abs() < 1e-6);
    }

    #[test]
    fn test_parse_lrc_lyrics_invalid_tags_sorting_and_trailing_notes() {
        let lrc = "Plain line with no timestamp
[unclosed bracket line
[not_a_time] Invalid non-colon tag
[ar:Some Artist]
[ti:Some Title]
[offset:invalid_ms]
[-01:00.00] Negative minute
[00:-05.00] Negative second
[1:2:3:4] Too many colons
[00:30.00] Second chronologically
[00:15.00] First chronologically
[00:20.00]   
[00:40.00]
[00:45.00]   
";
        let parsed = parse_lrc_lyrics(lrc);
        // Trailing empty "♪" lines at 40s and 45s are stripped; middle "♪" at 20s is preserved
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].text, "First chronologically");
        assert!((parsed[0].time_secs.unwrap() - 15.0).abs() < 1e-6);
        assert_eq!(parsed[1].text, "♪");
        assert!((parsed[1].time_secs.unwrap() - 20.0).abs() < 1e-6);
        assert_eq!(parsed[2].text, "Second chronologically");
        assert!((parsed[2].time_secs.unwrap() - 30.0).abs() < 1e-6);
    }

    #[test]
    fn test_sanitize_track_title_for_lyrics() {
        assert_eq!(
            sanitize_track_title_for_lyrics("  Blinding Lights (Official Audio)  "),
            "Blinding Lights"
        );
        assert_eq!(
            sanitize_track_title_for_lyrics("Nightcall [official music video] [HD]"),
            "Nightcall"
        );
        assert_eq!(
            sanitize_track_title_for_lyrics("Resonance (Official Lyric Video) [4K]"),
            "Resonance"
        );
        assert_eq!(
            sanitize_track_title_for_lyrics("Song Name (Lyrics) [Audio]"),
            "Song Name"
        );
        assert_eq!(
            sanitize_track_title_for_lyrics("Midnight City (Eric Prydz Remix)"),
            "Midnight City (Eric Prydz Remix)"
        );
    }

    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("MV_3Dpw-BRY", 32));
        assert!(is_valid_identifier("VLPL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i", 64));
        assert!(!is_valid_identifier("", 32));
        assert!(!is_valid_identifier("abcde", 4));
        assert!(is_valid_identifier("abcd", 4));
        assert!(!is_valid_identifier("id with space", 32));
        assert!(!is_valid_identifier("id/path?x=1", 32));
        assert!(!is_valid_identifier("música", 32));
    }

    #[test]
    fn test_deduplicate_tracks_and_playlists() {
        let mk_track = |vid: &str, title: &str| TrackItem {
            video_id: vid.to_string(),
            title: title.to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_text: "3:00".to_string(),
            duration_secs: 180.0,
            thumbnail_url: String::new(),
        };
        let mut tracks = vec![
            mk_track("v1", "First v1"),
            mk_track("v2", "First v2"),
            mk_track("v1", "Duplicate v1"),
            mk_track("v3", "First v3"),
            mk_track("v2", "Duplicate v2"),
        ];
        deduplicate_tracks(&mut tracks);
        let ids: Vec<&str> = tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids, vec!["v1", "v2", "v3"]);
        assert_eq!(tracks[0].title, "First v1");

        let mk_pl = |pid: &str, title: &str| PlaylistItem {
            playlist_id: pid.to_string(),
            title: title.to_string(),
            subtitle: String::new(),
            track_count_text: String::new(),
            thumbnail_url: String::new(),
        };
        let mut playlists = vec![
            mk_pl("PL1", "One"),
            mk_pl("PL2", "Two"),
            mk_pl("PL1", "One dup"),
        ];
        deduplicate_playlists(&mut playlists);
        let pids: Vec<&str> = playlists.iter().map(|p| p.playlist_id.as_str()).collect();
        assert_eq!(pids, vec!["PL1", "PL2"]);
    }

    #[test]
    fn test_parse_lrclib_record_synced_plain_and_empty() {
        // Synced lyrics take precedence over plainLyrics
        let synced_record = json!({
            "syncedLyrics": "[00:05.00] Synced line 1
[00:10.00] Synced line 2",
            "plainLyrics": "Plain line 1
Plain line 2"
        });
        let (lines, is_synced) = parse_lrclib_record(&synced_record).unwrap();
        assert!(is_synced);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Synced line 1");
        assert_eq!(lines[0].time_secs, Some(5.0));

        // Falls back to plainLyrics if syncedLyrics yields no valid timestamps
        let fallback_record = json!({
            "syncedLyrics": "[ar:Only Metadata]",
            "plainLyrics": "  First plain line 

  Second plain line  
"
        });
        let (plain_lines, is_synced_fb) = parse_lrclib_record(&fallback_record).unwrap();
        assert!(!is_synced_fb);
        assert_eq!(plain_lines.len(), 2);
        assert_eq!(plain_lines[0].text, "First plain line");
        assert_eq!(plain_lines[0].time_secs, None);
        assert_eq!(plain_lines[1].text, "Second plain line");

        // Empty / missing lyrics returns None
        let empty_record = json!({
            "syncedLyrics": "",
            "plainLyrics": "   
  "
        });
        assert!(parse_lrclib_record(&empty_record).is_none());
        assert!(parse_lrclib_record(&json!({})).is_none());
    }

    #[test]
    fn test_parse_responsive_list_item_track_and_edge_cases() {
        let renderer = json!({
            "playlistItemData": { "videoId": "MV_3Dpw-BRY" },
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Nightcall" }] }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [
                                { "text": "Kavinsky" },
                                { "text": " • " },
                                { "text": "OutRun" },
                                { "text": " • " },
                                { "text": "4:18" }
                            ]
                        }
                    }
                }
            ],
            "thumbnail": {
                "musicThumbnailRenderer": {
                    "thumbnail": {
                        "thumbnails": [
                            { "url": "https://lh3.googleusercontent.com/small.jpg" },
                            { "url": "https://lh3.googleusercontent.com/large.jpg" }
                        ]
                    }
                }
            }
        });

        let track = parse_responsive_list_item_track(&renderer).expect("valid track should parse");
        assert_eq!(track.video_id, "MV_3Dpw-BRY");
        assert_eq!(track.title, "Nightcall");
        assert_eq!(track.artist, "Kavinsky");
        assert_eq!(track.album, "OutRun");
        assert_eq!(track.duration_text, "4:18");
        assert!((track.duration_secs - 258.0).abs() < 1e-6);
        assert_eq!(
            track.thumbnail_url,
            "https://lh3.googleusercontent.com/large.jpg"
        );

        // Fixed column duration + missing artist defaults
        let minimal_renderer = json!({
            "playlistItemData": { "videoId": "abc12345678" },
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Instrumental" }] }
                    }
                }
            ],
            "fixedColumns": [
                {
                    "musicResponsiveListItemFixedColumnRenderer": {
                        "text": { "runs": [{ "text": "2:05" }] }
                    }
                }
            ]
        });
        let minimal = parse_responsive_list_item_track(&minimal_renderer).unwrap();
        assert_eq!(minimal.artist, "YouTube Music");
        assert_eq!(minimal.duration_text, "2:05");
        assert!((minimal.duration_secs - 125.0).abs() < 1e-6);
        assert_eq!(
            minimal.thumbnail_url,
            "https://i.ytimg.com/vi/abc12345678/hqdefault.jpg"
        );

        // Invalid renderers return None
        assert!(parse_responsive_list_item_track(&json!({})).is_none());
        assert!(parse_responsive_list_item_track(&json!({
            "playlistItemData": { "videoId": "   " },
            "flexColumns": []
        }))
        .is_none());
        assert!(parse_responsive_list_item_track(&json!({
            "playlistItemData": { "videoId": "valid123456" },
            "flexColumns": [{
                "musicResponsiveListItemFlexColumnRenderer": {
                    "text": { "runs": [{ "text": "   " }] }
                }
            }]
        }))
        .is_none());
    }

    #[test]
    fn test_extract_home_sections_from_innertube_fixture() {
        let fixture = json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": { "runs": [{ "text": "Quick Picks" }] }
                                                    }
                                                },
                                                "contents": [
                                                    {
                                                        "musicResponsiveListItemRenderer": {
                                                            "playlistItemData": { "videoId": "vid00000001" },
                                                            "flexColumns": [
                                                                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Track One" }] } } },
                                                                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Artist A" }, { "text": "3:10" }] } } }
                                                            ]
                                                        }
                                                    },
                                                    {
                                                        // Duplicate video_id in same shelf should be deduplicated
                                                        "musicResponsiveListItemRenderer": {
                                                            "playlistItemData": { "videoId": "vid00000001" },
                                                            "flexColumns": [
                                                                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Track One Dup" }] } } }
                                                            ]
                                                        }
                                                    },
                                                    {
                                                        "musicTwoRowItemRenderer": {
                                                            "navigationEndpoint": { "watchEndpoint": { "videoId": "vid00000002" } },
                                                            "title": { "runs": [{ "text": "Track Two" }] },
                                                            "subtitle": { "runs": [{ "text": "Song" }, { "text": " • " }, { "text": "Artist B" }] },
                                                            "thumbnailOverlay": {
                                                                "badge": { "text": "4:05" }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        {
                                            "musicShelfRenderer": {
                                                "title": { "runs": [{ "text": "Empty Shelf Ignored" }] },
                                                "contents": []
                                            }
                                        }
                                    ]
                                }
                            }
                        }
                    }]
                }
            }
        });

        let sections = extract_home_sections(&fixture);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "Quick Picks");
        assert_eq!(sections[0].items.len(), 2);
        assert_eq!(sections[0].items[0].video_id, "vid00000001");
        assert_eq!(sections[0].items[0].artist, "Artist A");
        assert_eq!(sections[0].items[1].video_id, "vid00000002");
        assert_eq!(sections[0].items[1].artist, "Artist B");
        assert_eq!(sections[0].items[1].duration_text, "4:05");

        // Fallback flat track extraction when no shelf renderer is present
        let flat_fixture = json!({
            "items": [{
                "playlistPanelVideoRenderer": {
                    "videoId": "vid00000003",
                    "title": { "simpleText": "Radio Track" },
                    "longBylineText": { "runs": [{ "text": "Radio Artist" }] },
                    "lengthText": { "simpleText": "1:02:03" }
                }
            }]
        });
        let flat_sections = extract_home_sections(&flat_fixture);
        assert_eq!(flat_sections.len(), 1);
        assert_eq!(flat_sections[0].title, "Destaques do YouTube Music");
        assert_eq!(flat_sections[0].items.len(), 1);
        assert!((flat_sections[0].items[0].duration_secs - 3723.0).abs() < 1e-6);

        assert!(extract_home_sections(&json!({})).is_empty());
    }

    #[test]
    fn test_resolve_search_filter_params_and_duration_parsing() {
        assert_eq!(resolve_search_filter_params(None), None);
        assert_eq!(resolve_search_filter_params(Some("all")), None);
        assert_eq!(resolve_search_filter_params(Some("tudo")), None);
        assert!(resolve_search_filter_params(Some("songs")).is_some());
        assert!(resolve_search_filter_params(Some(" Músicas ")).is_some());
        assert!(resolve_search_filter_params(Some("videos")).is_some());
        assert!(resolve_search_filter_params(Some("álbuns")).is_some());
        assert!(resolve_search_filter_params(Some("artistas")).is_some());
        assert!(resolve_search_filter_params(Some("playlists")).is_some());
        assert_eq!(resolve_search_filter_params(Some("unknown")), None);

        assert_eq!(parse_duration_secs("3:45"), Some(225.0));
        assert_eq!(parse_duration_secs("1:02:03"), Some(3723.0));
        assert_eq!(parse_duration_secs("0:00"), None);
        assert_eq!(parse_duration_secs("45"), None);
        assert_eq!(parse_duration_secs("3:ab"), None);
        assert!(looks_like_duration("4:18"));
        assert!(!looks_like_duration("Kavinsky"));
    }
}
