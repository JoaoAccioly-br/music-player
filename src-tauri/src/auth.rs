use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const YTM_ORIGIN: &str = "https://music.youtube.com";
pub const GOOGLE_LOGIN_URL: &str = "https://accounts.google.com/ServiceLogin?service=youtube&uilel=3&passive=true&continue=https%3A%2F%2Fmusic.youtube.com%2F";
pub const FIREFOX_LINUX_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0";

/// Internal session persisted to `~/.local/share/com.ytmusic.rust/auth.json` with `0600` permissions.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AuthSession {
    pub is_authenticated: bool,
    pub cookie_header: String,
    pub sapisid: String,
    pub auth_method: String,
    pub updated_at: u64,
}

/// Safe public status sent to the Vue 3 frontend (masks raw cookies while exposing status and preview).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AuthStatus {
    pub is_authenticated: bool,
    pub sapisid_preview: String,
    pub account_hint: String,
    pub auth_method: String,
    pub has_sapisid: bool,
    pub updated_at: u64,
}

impl AuthSession {
    pub fn to_status(&self) -> AuthStatus {
        let preview = if self.sapisid.len() > 8 {
            format!(
                "{}...{}",
                &self.sapisid[..4],
                &self.sapisid[self.sapisid.len() - 4..]
            )
        } else if !self.sapisid.is_empty() {
            "***".to_string()
        } else {
            String::new()
        };

        let hint = if self.is_authenticated && !self.sapisid.is_empty() {
            format!("SAPISID ({preview})")
        } else {
            "Não conectado".to_string()
        };

        AuthStatus {
            is_authenticated: self.is_authenticated && !self.sapisid.is_empty(),
            sapisid_preview: preview,
            account_hint: hint,
            auth_method: self.auth_method.clone(),
            has_sapisid: !self.sapisid.is_empty(),
            updated_at: self.updated_at,
        }
    }
}

/// Extracts `SAPISID` or `__Secure-3PAPISID` from:
/// 1. Standard HTTP Cookie header (`key=val; SAPISID=xxx; ...`)
/// 2. Netscape cookies.txt lines (`domain \t flag \t path \t secure \t exp \t name \t value`)
/// 3. Direct `SAPISID` token pasted by the user (`xxx/yyy`)
pub fn extract_sapisid_from_cookie(cookie_str: &str) -> Option<String> {
    let trimmed = cookie_str.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Check Netscape format lines (tab-separated)
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 7 {
            let name = cols[5].trim();
            let val = cols[6].trim();
            if (name == "__Secure-3PAPISID" || name == "SAPISID") && !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }

    // 2. Check standard `key=value; key2=value2` format
    let without_prefix = trimmed
        .strip_prefix("Cookie:")
        .or_else(|| trimmed.strip_prefix("cookie:"))
        .unwrap_or(trimmed)
        .trim();

    let mut fallback_sapisid: Option<String> = None;
    for part in without_prefix.split(';') {
        let pair = part.trim();
        if let Some((k, v)) = pair.split_once('=') {
            let key = k.trim();
            let val = v.trim().trim_matches('"');
            if key == "__Secure-3PAPISID" && !val.is_empty() {
                return Some(val.to_string());
            }
            if (key == "SAPISID" || key == "__Secure-1PAPISID") && !val.is_empty() {
                fallback_sapisid = Some(val.to_string());
            }
        }
    }

    if fallback_sapisid.is_some() {
        return fallback_sapisid;
    }

    // 3. If the user pasted the raw SAPISID token value directly (e.g. `AbCdEf123456/GhIjKlMnOpQrStUv`)
    if !without_prefix.contains('=')
        && !without_prefix.contains(';')
        && !without_prefix.contains(' ')
        && without_prefix.len() >= 14
    {
        return Some(without_prefix.trim_matches('"').to_string());
    }

    None
}

/// Normalizes any supported cookie input (Cookie header, Netscape format, or raw SAPISID token)
/// into a clean HTTP `Cookie` header string guaranteed to include `SAPISID` and `__Secure-3PAPISID`.
pub fn normalize_cookie_header(cookie_str: &str, sapisid: &str) -> String {
    let trimmed = cookie_str.trim();

    // Check if it's Netscape format
    let mut netscape_pairs = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 7 {
            let name = cols[5].trim();
            let val = cols[6].trim();
            if !name.is_empty() && !val.is_empty() {
                netscape_pairs.push(format!("{name}={val}"));
            }
        }
    }
    if !netscape_pairs.is_empty() {
        let joined = netscape_pairs.join("; ");
        return ensure_sapisid_keys(&joined, sapisid);
    }

    let without_prefix = trimmed
        .strip_prefix("Cookie:")
        .or_else(|| trimmed.strip_prefix("cookie:"))
        .unwrap_or(trimmed)
        .trim();

    if !without_prefix.contains('=') {
        return format!(
            "SAPISID={sapisid}; __Secure-3PAPISID={sapisid}; __Secure-1PAPISID={sapisid}; CONSENT=YES+cb; PREF=f6=40000000&hl=pt&gl=BR"
        );
    }

    ensure_sapisid_keys(without_prefix, sapisid)
}

fn ensure_sapisid_keys(header: &str, sapisid: &str) -> String {
    let mut parts: Vec<String> = header
        .split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let has_sapisid = parts.iter().any(|p| p.starts_with("SAPISID="));
    let has_3papisid = parts.iter().any(|p| p.starts_with("__Secure-3PAPISID="));

    if !has_sapisid {
        parts.push(format!("SAPISID={sapisid}"));
    }
    if !has_3papisid {
        parts.push(format!("__Secure-3PAPISID={sapisid}"));
    }

    parts.join("; ")
}

/// Generates the `Authorization` header required by YouTube Music's Innertube `WEB_REMIX` API:
/// `SAPISIDHASH <timestamp>_<sha1(timestamp + " " + sapisid + " " + origin)>`
pub fn generate_sapisidhash(sapisid: &str, origin: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs();

    let input = format!("{timestamp} {sapisid} {origin}");
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();

    let mut hash_hex = String::with_capacity(40);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut hash_hex, "{byte:02x}");
    }

    format!(
        "SAPISIDHASH {timestamp}_{hash_hex} SAPISID1PHASH {timestamp}_{hash_hex} SAPISID3PHASH {timestamp}_{hash_hex}"
    )
}

/// Resolves `~/.local/share/com.ytmusic.rust/auth.json`.
pub fn get_auth_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("com.ytmusic.rust")
        .join("auth.json")
}

/// Loads the persisted `AuthSession` from `~/.local/share/com.ytmusic.rust/auth.json` if present.
pub fn load_session_from_disk() -> AuthSession {
    let path = get_auth_file_path();
    let Ok(contents) = fs::read_to_string(&path) else {
        return AuthSession::default();
    };
    let Ok(mut session) = serde_json::from_str::<AuthSession>(&contents) else {
        return AuthSession::default();
    };

    if session.sapisid.is_empty() {
        if let Some(extracted) = extract_sapisid_from_cookie(&session.cookie_header) {
            session.sapisid = extracted;
        }
    }

    session.is_authenticated = !session.sapisid.is_empty() && !session.cookie_header.is_empty();
    session
}

/// Persists `AuthSession` to `~/.local/share/com.ytmusic.rust/auth.json` with strict `0700` dir
/// and `0600` file permissions on Unix so no other local user/process can read the session cookies.
pub fn save_session_to_disk(session: &AuthSession) -> Result<(), String> {
    let path = get_auth_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Falha ao criar diretório de sessão {:?}: {e}", parent))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let serialized = serde_json::to_string_pretty(session)
        .map_err(|e| format!("Falha ao serializar sessão: {e}"))?;

    fs::write(&path, serialized)
        .map_err(|e| format!("Falha ao gravar arquivo de sessão {:?}: {e}", path))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Falha ao aplicar permissão 0600 em {:?}: {e}", path))?;
    }

    Ok(())
}

/// Deletes `~/.local/share/com.ytmusic.rust/auth.json` from disk.
pub fn clear_session_on_disk() -> Result<(), String> {
    let path = get_auth_file_path();
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("Falha ao remover arquivo de sessão {:?}: {e}", path))?;
    }
    Ok(())
}

/// Builds an authenticated `AuthSession` from a raw cookie string or `SAPISID` token.
pub fn build_session_from_cookie_str(
    cookie_str: &str,
    method: &str,
) -> Result<AuthSession, String> {
    let Some(sapisid) = extract_sapisid_from_cookie(cookie_str) else {
        return Err(
            "Cookie inválido: não foi encontrado o token 'SAPISID' ou '__Secure-3PAPISID'. Cole o cabeçalho Cookie completo do music.youtube.com ou o valor do SAPISID."
                .to_string(),
        );
    };

    let normalized_header = normalize_cookie_header(cookie_str, &sapisid);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs();

    Ok(AuthSession {
        is_authenticated: true,
        cookie_header: normalized_header,
        sapisid,
        auth_method: method.to_string(),
        updated_at: now,
    })
}

/// Opens a temporary secondary WebView window (`"google-login"`) pointing to Google Login -> YouTube Music.
/// Uses a clean Firefox Linux Desktop User-Agent to avoid Google's embedded browser block on WebKitGTK,
/// and starts a background cookie watcher that automatically captures `SAPISID`, saves `auth.json` (`0600`),
/// and destroys the login WebView as soon as login completes to free 100% of its RAM!
pub fn open_google_login_window(app: &AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("google-login") {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    let parsed_url: tauri::Url = GOOGLE_LOGIN_URL
        .parse()
        .map_err(|e| format!("URL de login inválida: {e}"))?;

    let login_window =
        WebviewWindowBuilder::new(app, "google-login", WebviewUrl::External(parsed_url))
            .title("Conectar Conta Google — YouTube Music (Janela Temporária Zero-RAM)")
            .inner_size(520.0, 680.0)
            .resizable(true)
            .user_agent(FIREFOX_LINUX_UA)
            .build()
            .map_err(|e| format!("Falha ao abrir janela de login Google: {e}"))?;

    let _ = login_window.show();
    let _ = login_window.set_focus();

    spawn_login_cookie_watcher(app.clone());
    Ok(())
}

/// Polls the temporary `"google-login"` WebView window's cookie store every 1500ms while open.
/// As soon as `SAPISID` or `__Secure-3PAPISID` is detected, persists the session, updates `AppContainer`,
/// emits `auth-state-changed`, and destroys the `"google-login"` window to free RAM.
fn spawn_login_cookie_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Ok(ytm_url) = "https://music.youtube.com".parse::<tauri::Url>() else {
            return;
        };
        let Ok(yt_url) = "https://www.youtube.com".parse::<tauri::Url>() else {
            return;
        };

        loop {
            tokio::time::sleep(Duration::from_millis(1500)).await;

            let Some(login_win) = app.get_webview_window("google-login") else {
                // User closed the login window manually
                break;
            };

            let mut cookie_pairs: Vec<String> = Vec::new();

            if let Ok(cookies) = login_win.cookies_for_url(ytm_url.clone()) {
                for c in cookies {
                    cookie_pairs.push(format!("{}={}", c.name(), c.value()));
                }
            }
            if let Ok(cookies) = login_win.cookies_for_url(yt_url.clone()) {
                for c in cookies {
                    let entry = format!("{}={}", c.name(), c.value());
                    if !cookie_pairs.contains(&entry) {
                        cookie_pairs.push(entry);
                    }
                }
            }

            if cookie_pairs.is_empty() {
                continue;
            }

            let combined = cookie_pairs.join("; ");
            // Wait until both SAPISID and a session identifier (like SID, HSID, __Secure-1PSID, or LOGIN_INFO) exist
            let has_login_cookie = combined.contains("SID=")
                || combined.contains("__Secure-1PSID=")
                || combined.contains("__Secure-3PSID=")
                || combined.contains("LOGIN_INFO=");

            if has_login_cookie {
                if let Ok(session) = build_session_from_cookie_str(&combined, "webview") {
                    let _ = save_session_to_disk(&session);
                    let status = session.to_status();

                    if let Some(container) = app.try_state::<crate::AppContainer>() {
                        if let Ok(mut auth_guard) = container.auth.lock() {
                            *auth_guard = session;
                        }
                    }

                    let _ = app.emit("auth-state-changed", &status);
                    // Destroy the temporary login WebView immediately to reclaim RAM
                    let _ = login_win.close();
                    break;
                }
            }
        }
    });
}
