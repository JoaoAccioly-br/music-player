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

/// Resolves `<app_data_dir>/auth.json`.
pub fn get_auth_file_path() -> PathBuf {
    crate::persistence::get_app_data_dir().join("auth.json")
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
/// and `0600` file permissions on Unix without TOCTOU race conditions.
pub fn save_session_to_disk(session: &AuthSession) -> Result<(), String> {
    let path = get_auth_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Falha ao criar diretório de sessão: {e}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let serialized =
        serde_json::to_string(session).map_err(|e| format!("Falha ao serializar sessão: {e}"))?;

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("Falha ao abrir arquivo de sessão protegido: {e}"))?;
        file.write_all(serialized.as_bytes())
            .map_err(|e| format!("Falha ao gravar arquivo de sessão: {e}"))?;
    }

    #[cfg(not(unix))]
    {
        fs::write(&path, serialized)
            .map_err(|e| format!("Falha ao gravar arquivo de sessão: {e}"))?;
    }

    Ok(())
}

/// Deletes `~/.local/share/com.ytmusic.rust/auth.json` from disk.
pub fn clear_session_on_disk() -> Result<(), String> {
    let path = get_auth_file_path();
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Falha ao remover arquivo de sessão: {e}"))?;
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
/// restricts navigation to verified Google / YouTube domains, and starts a background cookie watcher
/// that automatically captures `SAPISID`, saves `auth.json` (`0600`), and destroys the login WebView
/// as soon as login completes to free 100% of its RAM!
/// Validates whether a URL belongs to the Google / YouTube login navigation allowlist.
pub fn is_allowed_login_navigation_url(url: &tauri::Url) -> bool {
    let host = url.host_str().unwrap_or("");
    host == "accounts.google.com"
        || host.ends_with(".google.com")
        || host == "music.youtube.com"
        || host == "accounts.youtube.com"
        || host.ends_with(".youtube.com")
}

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
            .on_navigation(|url| is_allowed_login_navigation_url(url))
            .build()
            .map_err(|e| format!("Falha ao abrir janela de login Google: {e}"))?;

    let _ = login_window.show();
    let _ = login_window.set_focus();

    spawn_login_cookie_watcher(app.clone());
    Ok(())
}

/// Checks for an exact Google session cookie name (avoiding false positives like `SAPISID` matching `SID=`).
pub fn is_session_login_cookie_name(name: &str) -> bool {
    matches!(
        name,
        "SID" | "HSID" | "__Secure-1PSID" | "__Secure-3PSID" | "LOGIN_INFO"
    )
}

/// Polls the temporary `"google-login"` WebView window's cookie store every 1500ms while open.
/// As soon as `SAPISID` and a real Google session cookie are detected, persists the session,
/// updates `AppContainer`, emits `auth-state-changed`, and destroys the `"google-login"` window to free RAM.
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
            let mut has_login_cookie = false;

            if let Ok(cookies) = login_win.cookies_for_url(ytm_url.clone()) {
                for c in cookies {
                    if is_session_login_cookie_name(c.name()) && !c.value().is_empty() {
                        has_login_cookie = true;
                    }
                    cookie_pairs.push(format!("{}={}", c.name(), c.value()));
                }
            }
            if let Ok(cookies) = login_win.cookies_for_url(yt_url.clone()) {
                for c in cookies {
                    if is_session_login_cookie_name(c.name()) && !c.value().is_empty() {
                        has_login_cookie = true;
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sapisid_from_cookie_formats() {
        assert_eq!(extract_sapisid_from_cookie(""), None);
        assert_eq!(extract_sapisid_from_cookie("   
	  "), None);
        assert_eq!(extract_sapisid_from_cookie("SID=abc; HSID=def"), None);
        assert_eq!(extract_sapisid_from_cookie("shorttoken"), None);

        // Standard Cookie header with prefix and quotes
        let header = "Cookie: SID=xyz; SAPISID=\"AbCdEf123456/GhIj\"; PREF=1";
        assert_eq!(
            extract_sapisid_from_cookie(header),
            Some("AbCdEf123456/GhIj".to_string())
        );

        // __Secure-3PAPISID takes priority over SAPISID
        let priority_header = "cookie: SAPISID=fallback_token_123; __Secure-3PAPISID=preferred_3p_token; __Secure-1PAPISID=onep_token";
        assert_eq!(
            extract_sapisid_from_cookie(priority_header),
            Some("preferred_3p_token".to_string())
        );

        // __Secure-1PAPISID fallback when SAPISID and __Secure-3PAPISID absent
        let onep_header = "SID=xyz; __Secure-1PAPISID=only_1p_token_value";
        assert_eq!(
            extract_sapisid_from_cookie(onep_header),
            Some("only_1p_token_value".to_string())
        );

        // Netscape cookies.txt format (ignoring comments & blank lines)
        let netscape = "# Netscape HTTP Cookie File

.youtube.com	TRUE	/	TRUE	2147483647	SID	sid_val
.youtube.com	TRUE	/	TRUE	2147483647	SAPISID	netscape_sapisid_999
";
        assert_eq!(
            extract_sapisid_from_cookie(netscape),
            Some("netscape_sapisid_999".to_string())
        );

        // Raw SAPISID token directly pasted (>= 14 chars, no '=', ';', ' ')
        assert_eq!(
            extract_sapisid_from_cookie("AbCdEf123456/GhIjKlMn"),
            Some("AbCdEf123456/GhIjKlMn".to_string())
        );
    }

    #[test]
    fn test_normalize_cookie_header_and_build_session() {
        // Raw token expands into full cookie header
        let normalized_raw = normalize_cookie_header("RawToken1234567890", "RawToken1234567890");
        assert!(normalized_raw.contains("SAPISID=RawToken1234567890"));
        assert!(normalized_raw.contains("__Secure-3PAPISID=RawToken1234567890"));
        assert!(normalized_raw.contains("__Secure-1PAPISID=RawToken1234567890"));

        // Standard header missing __Secure-3PAPISID gets it appended
        let normalized_hdr = normalize_cookie_header("Cookie: SID=1; SAPISID=tok123456789012", "tok123456789012");
        assert!(normalized_hdr.contains("SID=1"));
        assert!(normalized_hdr.contains("SAPISID=tok123456789012"));
        assert!(normalized_hdr.contains("__Secure-3PAPISID=tok123456789012"));

        // Netscape format converts to `; `-separated HTTP Cookie header
        let netscape = ".youtube.com	TRUE	/	TRUE	2147483647	SID	sid1
.youtube.com	TRUE	/	TRUE	2147483647	SAPISID	net_tok_12345678";
        let normalized_ns = normalize_cookie_header(netscape, "net_tok_12345678");
        assert!(normalized_ns.contains("SID=sid1"));
        assert!(normalized_ns.contains("SAPISID=net_tok_12345678"));
        assert!(normalized_ns.contains("__Secure-3PAPISID=net_tok_12345678"));

        // build_session_from_cookie_str
        assert!(build_session_from_cookie_str("SID=only_no_sapisid", "manual").is_err());
        let session = build_session_from_cookie_str("SAPISID=abcd1234efgh5678; SID=s1", "manual")
            .expect("valid cookie should build AuthSession");
        assert!(session.is_authenticated);
        assert_eq!(session.sapisid, "abcd1234efgh5678");
        assert_eq!(session.auth_method, "manual");

        let status = session.to_status();
        assert!(status.is_authenticated);
        assert!(status.has_sapisid);
        assert_eq!(status.sapisid_preview, "abcd...5678");
        assert_eq!(status.account_hint, "SAPISID (abcd...5678)");
    }

    #[test]
    fn test_auth_status_preview_for_short_and_empty_sapisid() {
        let short_session = AuthSession {
            is_authenticated: true,
            cookie_header: "SAPISID=short12".to_string(),
            sapisid: "short12".to_string(),
            auth_method: "manual".to_string(),
            updated_at: 100,
        };
        let status = short_session.to_status();
        assert_eq!(status.sapisid_preview, "***");
        assert_eq!(status.account_hint, "SAPISID (***)");

        let empty_status = AuthSession::default().to_status();
        assert!(!empty_status.is_authenticated);
        assert!(!empty_status.has_sapisid);
        assert_eq!(empty_status.sapisid_preview, "");
        assert_eq!(empty_status.account_hint, "Não conectado");
    }

    #[test]
    fn test_generate_sapisidhash_format_and_sha1_digest() {
        let sapisid = "TestSapisidToken/12345";
        let header = generate_sapisidhash(sapisid, YTM_ORIGIN);
        let parts: Vec<&str> = header.split(' ').collect();
        assert_eq!(parts.len(), 6, "Expected 3 scheme-value pairs: {header}");
        assert_eq!(parts[0], "SAPISIDHASH");
        assert_eq!(parts[2], "SAPISID1PHASH");
        assert_eq!(parts[4], "SAPISID3PHASH");
        assert_eq!(parts[1], parts[3]);
        assert_eq!(parts[1], parts[5]);

        let (ts_str, hex_hash) = parts[1]
            .split_once('_')
            .expect("hash value must contain '<ts>_<sha1>'");
        let _ts: u64 = ts_str.parse().expect("timestamp must be a valid u64");
        assert_eq!(hex_hash.len(), 40);
        assert!(hex_hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));

        // Independently verify SHA-1 digest matches RFC input "<ts> <sapisid> <origin>"
        let expected_input = format!("{ts_str} {sapisid} {YTM_ORIGIN}");
        let mut hasher = Sha1::new();
        hasher.update(expected_input.as_bytes());
        let expected_hex: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex_hash, expected_hex);
    }

    #[test]
    fn test_is_allowed_login_navigation_url_allowlist() {
        let allowed = [
            "https://accounts.google.com/ServiceLogin?service=youtube",
            "https://myaccount.google.com/security",
            "https://music.youtube.com/",
            "https://accounts.youtube.com/RotateCookies",
            "https://www.youtube.com/signin",
        ];
        for raw in allowed {
            let url: tauri::Url = raw.parse().unwrap();
            assert!(
                is_allowed_login_navigation_url(&url),
                "Expected URL to be allowed: {raw}"
            );
        }

        let blocked = [
            "https://evil.com/login",
            "https://google.com.evil.com/",
            "https://youtube.com.attacker.org/",
            "https://notgoogle.com/",
            "https://accounts.google.com.phish.io/",
            "about:blank",
            "data:text/html,<h1>phish</h1>",
            "file:///etc/passwd",
        ];
        for raw in blocked {
            let url: tauri::Url = raw.parse().unwrap();
            assert!(
                !is_allowed_login_navigation_url(&url),
                "Expected URL to be blocked: {raw}"
            );
        }
    }

    #[test]
    fn test_is_session_login_cookie_name_exact_match() {
        assert!(is_session_login_cookie_name("SID"));
        assert!(is_session_login_cookie_name("HSID"));
        assert!(is_session_login_cookie_name("__Secure-1PSID"));
        assert!(is_session_login_cookie_name("__Secure-3PSID"));
        assert!(is_session_login_cookie_name("LOGIN_INFO"));

        // Must NOT match SAPISID or partial substrings
        assert!(!is_session_login_cookie_name("SAPISID"));
        assert!(!is_session_login_cookie_name("__Secure-3PAPISID"));
        assert!(!is_session_login_cookie_name("PREF"));
    }
}
