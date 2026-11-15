use crate::innertube::TrackItem;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Lightweight player session persisted to `~/.local/share/com.ytmusic.rust/player_session.json`
/// with strict `0600` file permissions on Unix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub volume: f32,
    pub repeat_mode: String,
    pub autoplay_radio: bool,
    pub low_ram_no_images: bool,
    pub theme: String,
    pub queue: Vec<TrackItem>,
    pub queue_index: usize,
}

impl Default for PersistedSession {
    fn default() -> Self {
        Self {
            volume: 0.8,
            repeat_mode: "off".to_string(),
            autoplay_radio: true,
            low_ram_no_images: false,
            theme: "dark".to_string(),
            queue: Vec::new(),
            queue_index: 0,
        }
    }
}

/// Resolves the cross-platform application local data directory:
/// - Linux/BSD: `~/.local/share/com.ytmusic.rust` (or `$XDG_DATA_HOME/com.ytmusic.rust`)
/// - Windows: `%LOCALAPPDATA%\com.ytmusic.rust` (or `%APPDATA%\com.ytmusic.rust`)
/// - macOS: `~/Library/Application Support/com.ytmusic.rust`
pub fn get_app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            if !local_app_data.trim().is_empty() {
                return PathBuf::from(local_app_data).join("com.ytmusic.rust");
            }
        }
        if let Ok(app_data) = std::env::var("APPDATA") {
            if !app_data.trim().is_empty() {
                return PathBuf::from(app_data).join("com.ytmusic.rust");
            }
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            if !profile.trim().is_empty() {
                return PathBuf::from(profile)
                    .join("AppData")
                    .join("Local")
                    .join("com.ytmusic.rust");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("com.ytmusic.rust");
            }
        }
        return std::env::temp_dir().join("com.ytmusic.rust");
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("com.ytmusic.rust");
            }
        }
        return std::env::temp_dir().join("com.ytmusic.rust");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("com.ytmusic.rust");
            }
        }
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            if !xdg_data.trim().is_empty() {
                return PathBuf::from(xdg_data).join("com.ytmusic.rust");
            }
        }
        PathBuf::from(".")
            .join(".local")
            .join("share")
            .join("com.ytmusic.rust")
    }
}

/// Resolves `<app_data_dir>/player_session.json`.
pub fn get_session_file_path() -> PathBuf {
    get_app_data_dir().join("player_session.json")
}

/// Loads the persisted player session from `~/.local/share/com.ytmusic.rust/player_session.json` if present and valid.
/// Normalizes and clamps persisted session fields to valid bounds and defaults.
pub fn normalize_persisted_session(mut session: PersistedSession) -> PersistedSession {
    session.volume = session.volume.clamp(0.0, 1.0);
    session.repeat_mode = match session.repeat_mode.trim().to_lowercase().as_str() {
        "all" => "all".to_string(),
        "one" => "one".to_string(),
        _ => "off".to_string(),
    };
    session.theme = match session.theme.trim().to_lowercase().as_str() {
        "light" => "light".to_string(),
        _ => "dark".to_string(),
    };
    if !session.queue.is_empty() && session.queue_index >= session.queue.len() {
        session.queue_index = 0;
    }
    session
}

pub fn load_persisted_session() -> Option<PersistedSession> {
    let path = get_session_file_path();
    let contents = fs::read_to_string(&path).ok()?;
    let session = serde_json::from_str::<PersistedSession>(&contents).ok()?;
    Some(normalize_persisted_session(session))
}

/// Saves `PersistedSession` to `~/.local/share/com.ytmusic.rust/player_session.json` with `0700` directory
/// and `0600` file permissions on Unix without TOCTOU race condition.
pub fn save_persisted_session(session: &PersistedSession) {
    let path = get_session_file_path();
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let Ok(serialized) = serde_json::to_string(session) else {
        return;
    };

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut file) = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
        {
            let _ = file.write_all(serialized.as_bytes());
        }
    }

    #[cfg(not(unix))]
    {
        let _ = fs::write(&path, serialized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_track(id: &str) -> TrackItem {
        TrackItem {
            video_id: id.to_string(),
            title: format!("Title {id}"),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_text: "3:00".to_string(),
            duration_secs: 180.0,
            thumbnail_url: format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"),
        }
    }

    #[test]
    fn test_persisted_session_default_values() {
        let def = PersistedSession::default();
        assert!((def.volume - 0.8).abs() < f32::EPSILON);
        assert_eq!(def.repeat_mode, "off");
        assert!(def.autoplay_radio);
        assert!(!def.low_ram_no_images);
        assert_eq!(def.theme, "dark");
        assert!(def.queue.is_empty());
        assert_eq!(def.queue_index, 0);
    }

    #[test]
    fn test_normalize_persisted_session_clamps_volume() {
        let low = normalize_persisted_session(PersistedSession {
            volume: -0.75,
            ..Default::default()
        });
        assert_eq!(low.volume, 0.0);

        let high = normalize_persisted_session(PersistedSession {
            volume: 3.25,
            ..Default::default()
        });
        assert_eq!(high.volume, 1.0);

        let valid = normalize_persisted_session(PersistedSession {
            volume: 0.42,
            ..Default::default()
        });
        assert!((valid.volume - 0.42).abs() < 1e-6);
    }

    #[test]
    fn test_normalize_persisted_session_normalizes_repeat_mode_and_theme() {
        let s1 = normalize_persisted_session(PersistedSession {
            repeat_mode: "  ALL ".to_string(),
            theme: " LiGhT  ".to_string(),
            ..Default::default()
        });
        assert_eq!(s1.repeat_mode, "all");
        assert_eq!(s1.theme, "light");

        let s2 = normalize_persisted_session(PersistedSession {
            repeat_mode: "One".to_string(),
            theme: "DARK".to_string(),
            ..Default::default()
        });
        assert_eq!(s2.repeat_mode, "one");
        assert_eq!(s2.theme, "dark");

        let s3 = normalize_persisted_session(PersistedSession {
            repeat_mode: "shuffle_forever".to_string(),
            theme: "solarized".to_string(),
            ..Default::default()
        });
        assert_eq!(s3.repeat_mode, "off");
        assert_eq!(s3.theme, "dark");
    }

    #[test]
    fn test_normalize_persisted_session_resets_out_of_bounds_queue_index() {
        let queue = vec![sample_track("vid00000001"), sample_track("vid00000002")];

        let out_of_bounds = normalize_persisted_session(PersistedSession {
            queue: queue.clone(),
            queue_index: 2,
            ..Default::default()
        });
        assert_eq!(out_of_bounds.queue_index, 0);

        let way_out = normalize_persisted_session(PersistedSession {
            queue: queue.clone(),
            queue_index: 99,
            ..Default::default()
        });
        assert_eq!(way_out.queue_index, 0);

        let valid_idx = normalize_persisted_session(PersistedSession {
            queue,
            queue_index: 1,
            ..Default::default()
        });
        assert_eq!(valid_idx.queue_index, 1);

        let empty_queue = normalize_persisted_session(PersistedSession {
            queue: Vec::new(),
            queue_index: 4,
            ..Default::default()
        });
        assert_eq!(empty_queue.queue_index, 4);
    }
}
