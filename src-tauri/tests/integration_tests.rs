use std::ffi::OsString;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rodio::{Decoder, Source};
use youtube_music_rust_lib::audio::{
    clean_temp_cookie_files, get_temp_cookie_dir, is_valid_stream_url, parse_mp4_sidx_metadata,
    parse_youtube_video_id, write_temp_netscape_cookies, ProgressiveAudioBuffer,
    UNKNOWN_PROGRESSIVE_END_SENTINEL,
};
use youtube_music_rust_lib::auth::{
    build_session_from_cookie_str, clear_session_on_disk, extract_sapisid_from_cookie,
    generate_sapisidhash, get_auth_file_path, is_allowed_login_navigation_url,
    load_session_from_disk, save_session_to_disk, AuthSession, YTM_ORIGIN,
};
use youtube_music_rust_lib::innertube::{is_valid_identifier, parse_lrc_lyrics, TrackItem};
use youtube_music_rust_lib::persistence::{
    get_app_data_dir, get_session_file_path, load_persisted_session, normalize_persisted_session,
    save_persisted_session, PersistedSession,
};
use youtube_music_rust_lib::{determine_next_video_id, PlayerState, RepeatMode};

static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

struct HomeIsolationGuard<'a> {
    prev_home: Option<OsString>,
    #[cfg(all(unix, not(target_os = "macos")))]
    prev_xdg_data_home: Option<OsString>,
    #[cfg(windows)]
    prev_local_appdata: Option<OsString>,
    #[cfg(windows)]
    prev_appdata: Option<OsString>,
    #[cfg(windows)]
    prev_userprofile: Option<OsString>,
    temp_dir: PathBuf,
    _lock: MutexGuard<'a, ()>,
}

impl<'a> HomeIsolationGuard<'a> {
    fn new(test_name: &str) -> Self {
        let lock = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prev_home = std::env::var_os("HOME");
        #[cfg(all(unix, not(target_os = "macos")))]
        let prev_xdg_data_home = std::env::var_os("XDG_DATA_HOME");
        #[cfg(windows)]
        let prev_local_appdata = std::env::var_os("LOCALAPPDATA");
        #[cfg(windows)]
        let prev_appdata = std::env::var_os("APPDATA");
        #[cfg(windows)]
        let prev_userprofile = std::env::var_os("USERPROFILE");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp_dir = std::env::temp_dir().join(format!(
            "ytm_rust_integ_{}_{}_{}",
            test_name,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_dir).expect("should create isolated temp HOME directory");
        std::env::set_var("HOME", &temp_dir);
        #[cfg(all(unix, not(target_os = "macos")))]
        std::env::remove_var("XDG_DATA_HOME");
        #[cfg(windows)]
        {
            std::env::set_var("LOCALAPPDATA", &temp_dir);
            std::env::set_var("APPDATA", &temp_dir);
            std::env::set_var("USERPROFILE", &temp_dir);
        }
        Self {
            prev_home,
            #[cfg(all(unix, not(target_os = "macos")))]
            prev_xdg_data_home,
            #[cfg(windows)]
            prev_local_appdata,
            #[cfg(windows)]
            prev_appdata,
            #[cfg(windows)]
            prev_userprofile,
            temp_dir,
            _lock: lock,
        }
    }

    fn path(&self) -> &PathBuf {
        &self.temp_dir
    }
}

impl Drop for HomeIsolationGuard<'_> {
    fn drop(&mut self) {
        match &self.prev_home {
            Some(val) => std::env::set_var("HOME", val),
            None => std::env::remove_var("HOME"),
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        match &self.prev_xdg_data_home {
            Some(val) => std::env::set_var("XDG_DATA_HOME", val),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        #[cfg(windows)]
        {
            match &self.prev_local_appdata {
                Some(val) => std::env::set_var("LOCALAPPDATA", val),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
            match &self.prev_appdata {
                Some(val) => std::env::set_var("APPDATA", val),
                None => std::env::remove_var("APPDATA"),
            }
            match &self.prev_userprofile {
                Some(val) => std::env::set_var("USERPROFILE", val),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

fn build_test_sidx_header(timescale: u32, first_offset: u32, refs: &[(u32, u32)]) -> Vec<u8> {
    let ftyp_payload = b"M4A \x00\x00\x00\x00M4A mp42";
    let ftyp_len = (8 + ftyp_payload.len()) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(&ftyp_len.to_be_bytes());
    out.extend_from_slice(b"ftyp");
    out.extend_from_slice(ftyp_payload);

    let mut sidx_payload = Vec::new();
    sidx_payload.extend_from_slice(&[0u8, 0, 0, 0]); // version 0, flags 0
    sidx_payload.extend_from_slice(&1u32.to_be_bytes()); // reference_ID
    sidx_payload.extend_from_slice(&timescale.to_be_bytes());
    sidx_payload.extend_from_slice(&0u32.to_be_bytes()); // earliest_presentation_time
    sidx_payload.extend_from_slice(&first_offset.to_be_bytes());
    sidx_payload.extend_from_slice(&0u16.to_be_bytes()); // reserved
    sidx_payload.extend_from_slice(&(refs.len() as u16).to_be_bytes());
    for &(size, dur) in refs {
        sidx_payload.extend_from_slice(&size.to_be_bytes());
        sidx_payload.extend_from_slice(&dur.to_be_bytes());
        sidx_payload.extend_from_slice(&0u32.to_be_bytes());
    }
    let sidx_len = (8 + sidx_payload.len()) as u32;
    out.extend_from_slice(&sidx_len.to_be_bytes());
    out.extend_from_slice(b"sidx");
    out.extend_from_slice(&sidx_payload);
    out
}

/// Builds a valid stereo 16-bit PCM WAV container in memory so `ProgressiveAudioBuffer` +
/// `ReadSeekSource` + Symphonia `rodio::Decoder` can be tested end-to-end across Linux, Windows, and macOS
/// without requiring live network access or hardware audio output.
fn build_test_wav_pcm16_stereo(sample_rate: u32, num_frames: usize) -> Vec<u8> {
    let channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let block_align: u16 = channels * (bits_per_sample / 8);
    let byte_rate: u32 = sample_rate * (block_align as u32);
    let data_bytes_len: u32 = (num_frames as u32) * (block_align as u32);
    let riff_chunk_size: u32 = 36 + data_bytes_len;

    let mut wav = Vec::with_capacity((44 + data_bytes_len) as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_chunk_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat = 1 (PCM)
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes_len.to_le_bytes());

    for i in 0..num_frames {
        let sample_l = (((i * 97) % 16_000) as i16).wrapping_sub(8_000);
        let sample_r = (((i * 53) % 16_000) as i16).wrapping_sub(8_000);
        wav.extend_from_slice(&sample_l.to_le_bytes());
        wav.extend_from_slice(&sample_r.to_le_bytes());
    }
    wav
}

fn make_track(video_id: &str, title: &str, duration_secs: f64) -> TrackItem {
    TrackItem {
        video_id: video_id.to_string(),
        title: title.to_string(),
        artist: "Test Artist".to_string(),
        album: "Test Album".to_string(),
        duration_text: "3:20".to_string(),
        duration_secs,
        thumbnail_url: format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"),
    }
}

#[test]
fn test_public_api_lyrics_identifiers_and_player_queue_workflow() {
    let raw_url = "https://music.youtube.com/watch?v=MV_3Dpw-BRY&list=RDAMVM";
    let video_id = parse_youtube_video_id(raw_url);
    assert_eq!(video_id, "MV_3Dpw-BRY");
    assert!(is_valid_identifier(&video_id, 32));
    assert!(!is_valid_identifier("../etc/passwd", 32));

    let queue = vec![
        make_track(&video_id, "Nightcall", 258.0),
        make_track("fJ9rUzIMcZQ", "Bohemian Rhapsody", 355.0),
    ];

    let mut state = PlayerState::default();
    state.apply_track_from_queue(&queue, 0);
    state.is_playing = true;
    assert!(state.format_tray_status().contains("Nightcall"));

    assert_eq!(
        determine_next_video_id(&queue, 0, RepeatMode::Off),
        Some("fJ9rUzIMcZQ".to_string())
    );
    assert_eq!(
        determine_next_video_id(&queue, 1, RepeatMode::All),
        Some("MV_3Dpw-BRY".to_string())
    );
    assert_eq!(determine_next_video_id(&queue, 0, RepeatMode::One), None);

    let lrc = "\
[offset:500]
[00:01.00] Intro line
[00:05.50][00:15.50] Repeated chorus
[01:00:02.00] Late marker
[invalid:metadata] Ignored
";
    let parsed_lyrics = parse_lrc_lyrics(lrc);
    assert_eq!(parsed_lyrics.len(), 4);
    assert_eq!(parsed_lyrics[0].text, "Intro line");
    assert!((parsed_lyrics[0].time_secs.unwrap() - 0.5).abs() < 1e-6);
    assert_eq!(parsed_lyrics[1].text, "Repeated chorus");
    assert!((parsed_lyrics[1].time_secs.unwrap() - 5.0).abs() < 1e-6);
    assert_eq!(parsed_lyrics[2].text, "Repeated chorus");
    assert!((parsed_lyrics[2].time_secs.unwrap() - 15.0).abs() < 1e-6);
    assert_eq!(parsed_lyrics[3].text, "Late marker");
    assert!((parsed_lyrics[3].time_secs.unwrap() - 3601.5).abs() < 1e-6);
}

#[test]
fn test_progressive_audio_buffer_read_seek_with_sidx_metadata() {
    let header = build_test_sidx_header(44_100, 0, &[(32, 44_100), (32, 44_100)]);
    let header_len = header.len();
    let payload: Vec<u8> = (0u8..64).collect();
    let expected_total = (header_len + payload.len()) as u64;

    assert_eq!(
        parse_mp4_sidx_metadata(&header),
        Some((expected_total, 2.0))
    );

    let buffer = ProgressiveAudioBuffer::new(256, None);
    buffer.push_chunk(&header);
    assert_eq!(buffer.inspect_mp4_sidx(), Some((expected_total, 2.0)));

    let writer = buffer.clone();
    let writer_payload = payload.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        writer.push_chunk(&writer_payload[..32]);
        std::thread::sleep(Duration::from_millis(20));
        writer.push_chunk(&writer_payload[32..]);
        writer.mark_finished();
    });

    let mut reader = buffer.reader();
    // SeekFrom::End(0) resolves total expected length via sidx before download completes
    assert_eq!(reader.seek(SeekFrom::End(0)).unwrap(), expected_total);

    // Seek back to start of payload (past header), read sequentially across chunks
    assert_eq!(
        reader.seek(SeekFrom::Start(header_len as u64)).unwrap(),
        header_len as u64
    );
    let mut read_payload = Vec::new();
    reader.read_to_end(&mut read_payload).unwrap();
    handle.join().unwrap();

    assert_eq!(read_payload, payload);

    // Relative seek after mark_finished
    assert_eq!(
        reader.seek(SeekFrom::Current(-10)).unwrap(),
        expected_total - 10
    );
    let mut tail = [0u8; 10];
    reader.read_exact(&mut tail).unwrap();
    assert_eq!(tail, payload[54..64]);

    // Seek before start of stream must fail with InvalidInput
    assert_eq!(
        reader.seek(SeekFrom::Current(-5000)).unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
}

#[test]
fn test_progressive_audio_buffer_symphonia_decoder_and_seek_across_os() {
    // 2.0 seconds of stereo 44.1 kHz 16-bit PCM (88_200 frames = 176_400 samples = 352_800 data bytes > 64 KB MediaSourceStream buffer)
    let sample_rate = 44_100;
    let total_frames = 88_200;
    let wav_bytes = build_test_wav_pcm16_stereo(sample_rate, total_frames);

    // Create an in-flight ProgressiveAudioBuffer WITHOUT total_expected (simulating a non-DASH stream)
    let prog_buffer = ProgressiveAudioBuffer::new(wav_bytes.len(), None);
    // Push the initial 96 KB chunk (matching our 256 KB initial threshold in production, > 64 KB Symphonia ring buffer)
    // so the stream is still in-flight (`finished == false`)
    let initial_bytes = 96 * 1024;
    prog_buffer.push_chunk(&wav_bytes[..initial_bytes]);
    assert!(!prog_buffer.is_finished());

    // Verify SeekFrom::End(0) returns UNKNOWN_PROGRESSIVE_END_SENTINEL immediately without blocking
    let mut sentinel_check_reader = prog_buffer.reader();
    assert_eq!(
        sentinel_check_reader.seek(SeekFrom::End(0)).unwrap(),
        UNKNOWN_PROGRESSIVE_END_SENTINEL
    );

    // Initialize Symphonia `rodio::Decoder` while the stream is still in-flight!
    // If `ReadSeekSource::new()` did not filter out `UNKNOWN_PROGRESSIVE_END_SENTINEL`, or if
    // `SeekFrom::End(0)` blocked waiting for `mark_finished`, this would stall for 20s.
    let start_init = std::time::Instant::now();
    let mut decoder = Decoder::new(prog_buffer.reader()).expect(
        "Symphonia Decoder::new must initialize immediately on in-flight ProgressiveAudioBuffer",
    );
    assert!(
        start_init.elapsed() < Duration::from_secs(2),
        "Decoder::new must not block waiting for full stream completion"
    );
    assert_eq!(decoder.channels(), 2);
    assert_eq!(decoder.sample_rate(), sample_rate);

    // Spawn a background writer thread that feeds the rest of the audio chunks progressively
    let bg_writer = prog_buffer.clone();
    let remaining_bytes = wav_bytes[initial_bytes..].to_vec();
    let writer_handle = std::thread::spawn(move || {
        for chunk in remaining_bytes.chunks(16_384) {
            std::thread::sleep(Duration::from_millis(5));
            bg_writer.push_chunk(chunk);
        }
        bg_writer.mark_finished();
    });

    // Decode all interleaved samples across progressive chunk boundaries
    let decoded_samples: Vec<i16> = (&mut decoder).collect();
    writer_handle.join().unwrap();

    assert!(prog_buffer.is_finished());
    assert_eq!(
        decoded_samples.len(),
        total_frames * 2,
        "Decoder must yield all stereo samples across progressive chunks"
    );

    // Verify `try_seek` works accurately on a fresh Decoder over the completed buffer
    let mut seek_decoder = Decoder::new(prog_buffer.reader()).unwrap();
    seek_decoder
        .try_seek(Duration::from_millis(1000))
        .expect("Symphonia decoder try_seek must succeed");
    let remaining_after_seek: usize = seek_decoder.count();
    // Seeking to 1.0s of a 2.0s stereo stream should leave roughly half the samples (~88_200)
    assert!(
        remaining_after_seek >= 80_000 && remaining_after_seek <= 96_000,
        "Expected ~88200 samples after seeking to 1000ms, got {remaining_after_seek}"
    );
}

#[test]
fn test_persistence_session_disk_roundtrip_and_unix_permissions() {
    let guard = HomeIsolationGuard::new("persistence_roundtrip");

    // No session file initially
    assert!(load_persisted_session().is_none());

    let session_path = get_session_file_path();
    assert!(session_path.starts_with(guard.path()));

    let raw_session = PersistedSession {
        volume: 1.75,
        repeat_mode: " ALL ".to_string(),
        autoplay_radio: false,
        low_ram_no_images: true,
        theme: " LIGHT ".to_string(),
        queue: vec![
            make_track("MV_3Dpw-BRY", "Nightcall", 258.0),
            make_track("fJ9rUzIMcZQ", "Bohemian Rhapsody", 355.0),
        ],
        queue_index: 42,
        liked_tracks: vec![make_track("MV_3Dpw-BRY", "Nightcall", 258.0)],
    };

    save_persisted_session(&raw_session);
    assert!(session_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir_mode = fs::metadata(session_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&session_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "session dir must have 0700 permissions");
        assert_eq!(
            file_mode, 0o600,
            "player_session.json must have 0600 permissions"
        );
    }

    let loaded = load_persisted_session().expect("saved session must load from disk");
    assert!((loaded.volume - 1.0).abs() < f32::EPSILON);
    assert_eq!(loaded.repeat_mode, "all");
    assert!(!loaded.autoplay_radio);
    assert!(loaded.low_ram_no_images);
    assert_eq!(loaded.theme, "light");
    assert_eq!(loaded.queue.len(), 2);
    assert_eq!(loaded.queue_index, 0);
    assert_eq!(loaded.liked_tracks.len(), 1);
    assert_eq!(loaded.liked_tracks[0].video_id, "MV_3Dpw-BRY");

    // Pure normalization matches disk normalization
    assert_eq!(
        normalize_persisted_session(raw_session).repeat_mode,
        loaded.repeat_mode
    );

    // Corrupted JSON on disk gracefully returns None
    fs::write(&session_path, "{ not valid json").unwrap();
    assert!(load_persisted_session().is_none());
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn test_xdg_data_home_precedence_on_linux() {
    let guard = HomeIsolationGuard::new("xdg_precedence");
    let custom_xdg = guard.path().join("custom_xdg_data");
    std::env::set_var("XDG_DATA_HOME", &custom_xdg);
    let resolved = get_app_data_dir();
    assert_eq!(resolved, custom_xdg.join("com.ytmusic.rust"));
}

#[test]
fn test_auth_disk_persistence_and_public_helpers() {
    let guard = HomeIsolationGuard::new("auth_roundtrip");

    let auth_path = get_auth_file_path();
    assert!(auth_path.starts_with(guard.path()));
    assert!(!load_session_from_disk().is_authenticated);

    let raw_cookie = "SID=sid_abc; SAPISID=AuthToken123456/XYZ; HSID=hsid_123";
    assert_eq!(
        extract_sapisid_from_cookie(raw_cookie),
        Some("AuthToken123456/XYZ".to_string())
    );

    let session =
        build_session_from_cookie_str(raw_cookie, "manual").expect("valid cookie builds session");
    let auth_header = generate_sapisidhash(&session.sapisid, YTM_ORIGIN);
    assert!(auth_header.starts_with("SAPISIDHASH "));

    let login_url: tauri::Url = "https://accounts.google.com/ServiceLogin".parse().unwrap();
    assert!(is_allowed_login_navigation_url(&login_url));
    assert!(is_valid_stream_url(
        "https://rr2---sn-abc.googlevideo.com/videoplayback?itag=140"
    ));

    save_session_to_disk(&session).expect("saving auth.json must succeed");
    assert!(auth_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir_mode = fs::metadata(auth_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&auth_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "auth dir must have 0700 permissions");
        assert_eq!(file_mode, 0o600, "auth.json must have 0600 permissions");
    }

    let loaded = load_session_from_disk();
    assert!(loaded.is_authenticated);
    assert_eq!(loaded.sapisid, "AuthToken123456/XYZ");
    assert!(loaded.to_status().has_sapisid);

    // If sapisid was cleared in JSON, load_session_from_disk recovers it from cookie_header
    let mut missing_sapisid = loaded.clone();
    missing_sapisid.sapisid.clear();
    save_session_to_disk(&missing_sapisid).unwrap();
    let recovered = load_session_from_disk();
    assert!(recovered.is_authenticated);
    assert_eq!(recovered.sapisid, "AuthToken123456/XYZ");

    clear_session_on_disk().expect("clear_session_on_disk must succeed");
    assert!(!auth_path.exists());
    assert!(!load_session_from_disk().is_authenticated);
}

#[test]
fn test_write_and_clean_temp_netscape_cookies_sanitization_and_permissions() {
    let guard = HomeIsolationGuard::new("netscape_cookies");
    let cookie_dir = get_temp_cookie_dir();
    assert!(cookie_dir.starts_with(guard.path()));

    // Unauthenticated or empty sessions must not write any file
    assert!(write_temp_netscape_cookies(&AuthSession::default()).is_none());
    let empty_auth = AuthSession {
        is_authenticated: true,
        cookie_header: "   ".to_string(),
        ..Default::default()
    };
    assert!(write_temp_netscape_cookies(&empty_auth).is_none());

    // Inject \r, \n, and \t into cookie_header to verify strict Netscape sanitization
    let dirty_session = AuthSession {
        is_authenticated: true,
        cookie_header:
            "SAPISID=tok\r\n_clean\t123456; __Secure-3PAP\tISID=val\twith\rtabs\n; EMPTY="
                .to_string(),
        sapisid: "tok_clean123456".to_string(),
        auth_method: "manual".to_string(),
        updated_at: 1,
    };

    let cookie_path =
        write_temp_netscape_cookies(&dirty_session).expect("should write temp Netscape cookies");
    assert!(cookie_path.exists());
    let file_name = cookie_path.file_name().unwrap().to_string_lossy();
    assert!(file_name.starts_with("cookies.") && file_name.ends_with(".tmp.txt"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir_mode = fs::metadata(&cookie_dir).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(&cookie_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "cookie dir must have 0700 permissions");
        assert_eq!(
            file_mode, 0o600,
            "temp cookie file must have 0600 permissions"
        );
    }

    let content = fs::read_to_string(&cookie_path).unwrap();
    assert!(!content.contains('\r'), "CR characters must be stripped");

    let data_lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(
        data_lines.len(),
        2,
        "Injected newlines must not create extra Netscape cookie entries: {content}"
    );

    for line in &data_lines {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            cols.len(),
            7,
            "Each Netscape line must have exact 7 tab-separated columns (tabs stripped from name/value): {line}"
        );
        assert_eq!(cols[0], ".youtube.com");
    }
    assert_eq!(data_lines[0].split('\t').nth(5), Some("SAPISID"));
    assert_eq!(data_lines[0].split('\t').nth(6), Some("tok_clean123456"));
    assert_eq!(data_lines[1].split('\t').nth(5), Some("__Secure-3PAPISID"));
    assert_eq!(data_lines[1].split('\t').nth(6), Some("valwithtabs"));

    // Create an extra orphaned temp cookie file and a non-temp file that must be preserved
    let orphan_cookie = cookie_dir.join("cookies.99999.0.tmp.txt");
    fs::write(&orphan_cookie, "stale").unwrap();
    let keep_file = cookie_dir.join("keep_me.txt");
    fs::write(&keep_file, "important").unwrap();

    clean_temp_cookie_files();
    assert!(!cookie_path.exists());
    assert!(!orphan_cookie.exists());
    assert!(keep_file.exists());
}

#[test]
fn test_home_isolation_guard_restores_home_even_on_panic() {
    let sentinel = {
        let _lock = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::var_os("HOME")
    };

    let mut captured_temp_dir: Option<PathBuf> = None;
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let guard = HomeIsolationGuard::new("panic_restore");
        captured_temp_dir = Some(guard.path().clone());
        assert_eq!(
            std::env::var_os("HOME"),
            Some(guard.path().as_os_str().to_os_string())
        );
        panic!("simulated test failure");
    }));

    assert!(res.is_err());

    let _lock = HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(std::env::var_os("HOME"), sentinel);
    if let Some(dir) = captured_temp_dir {
        assert!(!dir.exists(), "temp HOME dir must be cleaned up on panic");
    }
}
