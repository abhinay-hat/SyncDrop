//! Google OAuth (loopback + PKCE) and Drive v3 upload.
//!
//! Client id lives in [`crate::config::ConfigStore`]. The client secret, refresh
//! token, and target folder name live in the OS keyring (service [`KEYRING_SERVICE`]).

use std::time::Duration;

use base64::Engine;

pub const KEYRING_SERVICE: &str = "SyncDrop";

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const USERINFO_ENDPOINT: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const DRIVE_FILES: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
/// `drive.file` plus the email scope the Swift client requested.
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file https://www.googleapis.com/auth/userinfo.email";
/// Multipart below this, resumable at or above. Matches the Swift client (`< 5_000_000`).
pub const MULTIPART_LIMIT: u64 = 5_000_000;
/// Local mtime must be this far ahead of Drive `modifiedTime` to count as changed.
const MTIME_SKEW_MS: i64 = 2_000;
const ACCOUNT_SECRET: &str = "google_client_secret";
const ACCOUNT_TOKENS: &str = "google_tokens";
const ACCOUNT_FOLDER: &str = "google_target_folder";
const AUTH_WAIT: Duration = Duration::from_secs(120);

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // ponytail: one timeout for the whole body. Chunked resume is the upgrade if multi-GB uploads die mid-PUT.
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| e.to_string())
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn clip(body: &str, status: u16) -> String {
    let trimmed: String = body.trim().chars().take(300).collect();
    if trimmed.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {trimmed}")
    }
}

mod oauth {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use std::io::Read;
    use std::net::TcpListener;
    use std::process::Command;
    use std::thread;
    use std::time::Instant;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GoogleTokens {
        pub access_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub refresh_token: Option<String>,
        pub expires_at: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub user_email: Option<String>,
    }

    impl GoogleTokens {
        /// Expired, or within 60 seconds of expiry (same skew as the Swift client).
        pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
            now >= self.expires_at - chrono::Duration::seconds(60)
        }

        pub fn is_expired(&self) -> bool {
            self.is_expired_at(Utc::now())
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum GoogleOAuthError {
        #[error("Please enter a Google OAuth Client ID.")]
        MissingClientId,
        #[error("Failed to start local redirect listener.")]
        ServerStartFailed,
        #[error("Sign-in was cancelled.")]
        UserCancelled,
        #[error("Authorization error: {0}")]
        AuthorizationFailed(String),
        #[error("Failed to exchange token: {0}")]
        TokenExchangeFailed(String),
        #[error("Not signed in to Google Drive.")]
        NotSignedIn,
        #[error("Received invalid response from Google.")]
        InvalidResponse,
        #[error("could not open the browser: {0}")]
        Browser(String),
        #[error("keyring: {0}")]
        Keyring(String),
        #[error("{0}")]
        Network(String),
    }

    /// S256 challenge for a PKCE verifier (RFC 7636).
    pub fn pkce_challenge(verifier: &str) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(verifier.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
    }

    /// 32 random bytes, base64url without padding, plus its S256 challenge.
    pub fn generate_pkce() -> (String, String) {
        use rand::RngCore;
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let challenge = pkce_challenge(&verifier);
        (verifier, challenge)
    }

    /// Path the loopback listener serves. The port is chosen when sign-in starts.
    pub const OAUTH_CALLBACK_PATH: &str = "/oauth2callback";

    /// Redirect URI to register for the desktop client. Sign-in appends the live port.
    pub fn oauth_redirect_uri() -> String {
        format!("http://127.0.0.1{OAUTH_CALLBACK_PATH}")
    }

    /// Installed-app authorize URL: loopback redirect, S256, offline consent.
    pub fn build_auth_url(
        client_id: &str,
        redirect_uri: &str,
        code_challenge: &str,
        state: &str,
    ) -> String {
        let params = [
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", DRIVE_SCOPE),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ];
        let mut url = String::from(AUTH_ENDPOINT);
        url.push('?');
        for (i, (key, value)) in params.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str(key);
            url.push('=');
            url.push_str(&percent_encode(value));
        }
        url
    }

    /// Token-endpoint JSON (`access_token`, `expires_in`, optional `refresh_token`).
    pub fn parse_token_response(
        body: &str,
        now: DateTime<Utc>,
    ) -> Result<GoogleTokens, GoogleOAuthError> {
        #[derive(Deserialize)]
        struct Raw {
            access_token: String,
            #[serde(default)]
            refresh_token: Option<String>,
            expires_in: i64,
        }
        let raw: Raw = serde_json::from_str(body).map_err(|_| GoogleOAuthError::InvalidResponse)?;
        if raw.access_token.is_empty() {
            return Err(GoogleOAuthError::InvalidResponse);
        }
        Ok(GoogleTokens {
            access_token: raw.access_token,
            refresh_token: raw.refresh_token.filter(|s| !s.is_empty()),
            expires_at: now + chrono::Duration::seconds(raw.expires_in),
            user_email: None,
        })
    }

    /// Apply a refresh-token response. Keeps the previous refresh token when Google omits a new one.
    pub fn apply_refresh_response(
        tokens: &mut GoogleTokens,
        body: &str,
        now: DateTime<Utc>,
    ) -> Result<(), GoogleOAuthError> {
        let parsed = parse_token_response(body, now)?;
        tokens.access_token = parsed.access_token;
        tokens.expires_at = parsed.expires_at;
        if parsed.refresh_token.is_some() {
            tokens.refresh_token = parsed.refresh_token;
        }
        Ok(())
    }

    /// First line of the loopback HTTP request. Returns the authorization code.
    pub fn parse_oauth_callback(
        request: &str,
        expected_state: &str,
    ) -> Result<String, GoogleOAuthError> {
        let first = request.lines().next().unwrap_or("");
        let target = first
            .split_whitespace()
            .nth(1)
            .ok_or(GoogleOAuthError::InvalidResponse)?;
        let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
        let pairs = query_pairs(query);
        if let Some(err) = pairs.iter().find(|(k, _)| k == "error").map(|(_, v)| v.clone()) {
            return Err(GoogleOAuthError::AuthorizationFailed(err));
        }
        let state = pairs
            .iter()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.as_str());
        if state != Some(expected_state) {
            return Err(GoogleOAuthError::AuthorizationFailed(
                "State mismatch.".into(),
            ));
        }
        pairs
            .into_iter()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v)
            .filter(|code| !code.is_empty())
            .ok_or(GoogleOAuthError::InvalidResponse)
    }

    fn query_pairs(query: &str) -> Vec<(String, String)> {
        query
            .split('&')
            .filter_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                Some((percent_decode(k), percent_decode(v)))
            })
            .collect()
    }

    /// Opens `url` with `open` / `xdg-open` / `cmd /C start`.
    pub fn open_system_browser(url: &str) -> Result<(), GoogleOAuthError> {
        browser_command(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| GoogleOAuthError::Browser(e.to_string()))
    }

    fn browser_command(url: &str) -> Command {
        #[cfg(target_os = "macos")]
        {
            let mut cmd = Command::new("open");
            cmd.arg(url);
            return cmd;
        }
        #[cfg(target_os = "windows")]
        {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "start", "", url]);
            return cmd;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let mut cmd = Command::new("xdg-open");
            cmd.arg(url);
            cmd
        }
    }

    /// OAuth session. Construct with [`GoogleOAuth::load`] so the secret and tokens come from the keyring.
    pub struct GoogleOAuth {
        client_id: String,
        client_secret: String,
        target_folder_name: String,
        tokens: Option<GoogleTokens>,
        http: reqwest::blocking::Client,
    }

    impl GoogleOAuth {
        /// Client id from `store` (already persisted in config.json). Secret, tokens, and folder name from the keyring.
        pub fn load(store: &crate::config::ConfigStore) -> Result<Self, GoogleOAuthError> {
            let http = http_client().map_err(GoogleOAuthError::Network)?;
            let client_secret = read_account(ACCOUNT_SECRET)?.unwrap_or_default();
            let target_folder_name = read_account(ACCOUNT_FOLDER)?
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "SyncDrop".to_string());
            let tokens = load_tokens()?;
            Ok(Self {
                client_id: store.google_client_id().trim().to_string(),
                client_secret,
                target_folder_name,
                tokens,
                http,
            })
        }

        pub fn client_id(&self) -> &str {
            &self.client_id
        }

        /// In-memory only. The caller saves this through [`crate::config::ConfigStore::set_google_client_id`].
        pub fn set_client_id(&mut self, id: impl Into<String>) {
            self.client_id = id.into().trim().to_string();
        }

        pub fn client_secret(&self) -> &str {
            &self.client_secret
        }

        pub fn set_client_secret(&mut self, secret: impl Into<String>) -> Result<(), GoogleOAuthError> {
            let trimmed = secret.into().trim().to_string();
            if trimmed.is_empty() {
                delete_account(ACCOUNT_SECRET)?;
            } else {
                write_account(ACCOUNT_SECRET, &trimmed)?;
            }
            self.client_secret = trimmed;
            Ok(())
        }

        pub fn target_folder_name(&self) -> &str {
            &self.target_folder_name
        }

        pub fn set_target_folder_name(
            &mut self,
            name: impl Into<String>,
        ) -> Result<(), GoogleOAuthError> {
            let trimmed = name.into().trim().to_string();
            let stored = if trimmed.is_empty() {
                "SyncDrop".to_string()
            } else {
                trimmed
            };
            write_account(ACCOUNT_FOLDER, &stored)?;
            self.target_folder_name = stored;
            Ok(())
        }

        pub fn tokens(&self) -> Option<&GoogleTokens> {
            self.tokens.as_ref()
        }

        pub fn user_email(&self) -> Option<&str> {
            self.tokens.as_ref().and_then(|t| t.user_email.as_deref())
        }

        pub fn is_signed_in(&self) -> bool {
            match &self.tokens {
                Some(tokens) => {
                    tokens
                        .refresh_token
                        .as_ref()
                        .is_some_and(|r| !r.is_empty())
                        || (!tokens.is_expired() && !tokens.access_token.is_empty())
                }
                None => false,
            }
        }

        /// Blocking installed-app flow. Waits up to 120 seconds for the browser to hit the loopback port.
        pub fn sign_in(&mut self) -> Result<(), GoogleOAuthError> {
            self.sign_in_with(open_system_browser)
        }

        /// Same as [`GoogleOAuth::sign_in`], but `open` receives the authorize URL (Tauri opener, tests).
        pub fn sign_in_with<F>(&mut self, open: F) -> Result<(), GoogleOAuthError>
        where
            F: FnOnce(&str) -> Result<(), GoogleOAuthError>,
        {
            let client_id = self.client_id.trim().to_string();
            if client_id.is_empty() {
                return Err(GoogleOAuthError::MissingClientId);
            }
            let (verifier, challenge) = generate_pkce();
            let state = uuid::Uuid::new_v4().to_string();
            let listener = TcpListener::bind("127.0.0.1:0")
                .map_err(|_| GoogleOAuthError::ServerStartFailed)?;
            let port = listener
                .local_addr()
                .map_err(|_| GoogleOAuthError::ServerStartFailed)?
                .port();
            let redirect = format!("http://127.0.0.1:{port}{OAUTH_CALLBACK_PATH}");
            let url = build_auth_url(&client_id, &redirect, &challenge, &state);
            open(&url)?;
            let code = accept_code(&listener, &state)?;
            let secret = self.client_secret.trim().to_string();
            let mut tokens = exchange_code(
                &self.http,
                &code,
                &verifier,
                &redirect,
                &client_id,
                &secret,
            )?;
            tokens.user_email = fetch_email(&self.http, &tokens.access_token)?;
            save_tokens(&tokens)?;
            self.tokens = Some(tokens);
            Ok(())
        }

        pub fn sign_out(&mut self) -> Result<(), GoogleOAuthError> {
            delete_account(ACCOUNT_TOKENS)?;
            self.tokens = None;
            Ok(())
        }

        pub fn get_valid_access_token(&mut self) -> Result<String, GoogleOAuthError> {
            if self.tokens.is_none() {
                self.tokens = load_tokens()?;
            }
            let Some(tokens) = self.tokens.clone() else {
                return Err(GoogleOAuthError::NotSignedIn);
            };
            if !tokens.is_expired() && !tokens.access_token.is_empty() {
                return Ok(tokens.access_token);
            }
            let refresh = tokens
                .refresh_token
                .clone()
                .filter(|s| !s.is_empty())
                .ok_or(GoogleOAuthError::NotSignedIn)?;
            let client_id = self.client_id.trim().to_string();
            if client_id.is_empty() {
                return Err(GoogleOAuthError::MissingClientId);
            }
            let secret = self.client_secret.trim().to_string();
            let body = form_body(&[
                ("client_id", client_id.as_str()),
                ("refresh_token", refresh.as_str()),
                ("grant_type", "refresh_token"),
            ], &secret);
            let text = post_token(&self.http, &body)?;
            let mut updated = tokens;
            apply_refresh_response(&mut updated, &text, Utc::now())?;
            save_tokens(&updated)?;
            let access = updated.access_token.clone();
            self.tokens = Some(updated);
            Ok(access)
        }
    }

    fn form_body(pairs: &[(&str, &str)], client_secret: &str) -> String {
        let mut parts: Vec<String> = pairs
            .iter()
            .map(|(k, v)| format!("{k}={}", percent_encode(v)))
            .collect();
        if !client_secret.is_empty() {
            parts.push(format!("client_secret={}", percent_encode(client_secret)));
        }
        parts.join("&")
    }

    fn post_token(http: &reqwest::blocking::Client, body: &str) -> Result<String, GoogleOAuthError> {
        let response = http
            .post(TOKEN_ENDPOINT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.to_string())
            .send()
            .map_err(|e| GoogleOAuthError::Network(e.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .map_err(|e| GoogleOAuthError::Network(e.to_string()))?;
        if !status.is_success() {
            return Err(GoogleOAuthError::TokenExchangeFailed(clip(
                &text,
                status.as_u16(),
            )));
        }
        Ok(text)
    }

    fn exchange_code(
        http: &reqwest::blocking::Client,
        code: &str,
        verifier: &str,
        redirect: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<GoogleTokens, GoogleOAuthError> {
        let body = form_body(
            &[
                ("code", code),
                ("client_id", client_id),
                ("redirect_uri", redirect),
                ("code_verifier", verifier),
                ("grant_type", "authorization_code"),
            ],
            client_secret,
        );
        let text = post_token(http, &body)?;
        parse_token_response(&text, Utc::now())
    }

    fn fetch_email(
        http: &reqwest::blocking::Client,
        access_token: &str,
    ) -> Result<Option<String>, GoogleOAuthError> {
        let response = http
            .get(USERINFO_ENDPOINT)
            .bearer_auth(access_token)
            .send()
            .map_err(|e| GoogleOAuthError::Network(e.to_string()))?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let text = response
            .text()
            .map_err(|e| GoogleOAuthError::Network(e.to_string()))?;
        #[derive(Deserialize)]
        struct UserInfo {
            email: Option<String>,
        }
        let info: UserInfo = serde_json::from_str(&text).unwrap_or(UserInfo { email: None });
        Ok(info.email.filter(|s| !s.is_empty()))
    }

    fn accept_code(listener: &TcpListener, expected_state: &str) -> Result<String, GoogleOAuthError> {
        listener
            .set_nonblocking(true)
            .map_err(|_| GoogleOAuthError::ServerStartFailed)?;
        let deadline = Instant::now() + AUTH_WAIT;
        // Keep accepting until a request carries the expected state (or we time out).
        // A local first-connect race must not steal the code or abort the wait.
        loop {
            if Instant::now() >= deadline {
                return Err(GoogleOAuthError::UserCancelled);
            }
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            stream.set_nonblocking(false).ok();
            stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
            let mut buf = [0u8; 8192];
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => {
                    let page = error_page("Sign-in could not be completed.");
                    let _ = write_oauth_page(&mut stream, &page);
                    continue;
                }
                Ok(n) => n,
            };
            let request = String::from_utf8_lossy(&buf[..n]);
            match parse_oauth_callback(&request, expected_state) {
                Ok(code) => {
                    let _ = write_oauth_page(&mut stream, &success_page());
                    return Ok(code);
                }
                Err(GoogleOAuthError::AuthorizationFailed(msg)) if msg == "State mismatch." => {
                    let _ = write_oauth_page(&mut stream, &error_page(&msg));
                    continue;
                }
                Err(GoogleOAuthError::AuthorizationFailed(msg)) => {
                    let _ = write_oauth_page(&mut stream, &error_page(&msg));
                    return Err(GoogleOAuthError::AuthorizationFailed(msg));
                }
                Err(err) => {
                    let _ = write_oauth_page(&mut stream, &error_page("Sign-in could not be completed."));
                    // Invalid / incomplete request from a racing client: keep listening.
                    let _ = err;
                    continue;
                }
            }
        }
    }

    fn write_oauth_page(stream: &mut impl std::io::Write, page: &str) -> std::io::Result<()> {
        let http = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        stream.write_all(http.as_bytes())
    }

    fn success_page() -> String {
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>SyncDrop — Google Drive Connected</title></head><body><h1>Google Drive Connected</h1><p>You can close this tab and return to SyncDrop.</p></body></html>".to_string()
    }

    fn error_page(msg: &str) -> String {
        format!(
            "<!DOCTYPE html><html><body><h1>Authorization Failed</h1><p>{}</p><p>You can close this window and try again in SyncDrop.</p></body></html>",
            html_escape(msg)
        )
    }

    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn entry(account: &str) -> Result<keyring::Entry, GoogleOAuthError> {
        keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|e| GoogleOAuthError::Keyring(e.to_string()))
    }

    fn read_account(account: &str) -> Result<Option<String>, GoogleOAuthError> {
        match entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(GoogleOAuthError::Keyring(err.to_string())),
        }
    }

    fn write_account(account: &str, value: &str) -> Result<(), GoogleOAuthError> {
        entry(account)?
            .set_password(value)
            .map_err(|e| GoogleOAuthError::Keyring(e.to_string()))
    }

    fn delete_account(account: &str) -> Result<(), GoogleOAuthError> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(GoogleOAuthError::Keyring(err.to_string())),
        }
    }

    fn save_tokens(tokens: &GoogleTokens) -> Result<(), GoogleOAuthError> {
        let json = serde_json::to_string(tokens).map_err(|_| GoogleOAuthError::InvalidResponse)?;
        write_account(ACCOUNT_TOKENS, &json)
    }

    fn load_tokens() -> Result<Option<GoogleTokens>, GoogleOAuthError> {
        match read_account(ACCOUNT_TOKENS)? {
            None => Ok(None),
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|_| GoogleOAuthError::InvalidResponse),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn pkce_challenge_matches_rfc7636_and_generator_is_url_safe() {
            let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
            assert_eq!(
                pkce_challenge(verifier),
                "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
            );
            let (generated, challenge) = generate_pkce();
            assert_eq!(generated.len(), 43);
            assert!(generated
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            assert_eq!(challenge, pkce_challenge(&generated));
        }

        #[test]
        fn auth_url_carries_pkce_offline_consent() {
            let url = build_auth_url(
                "my-client.apps.googleusercontent.com",
                "http://127.0.0.1:1234/oauth2callback",
                "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
                "state-1",
            );
            assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
            assert!(url.contains("client_id=my-client.apps.googleusercontent.com"));
            assert!(url.contains(
                "redirect_uri=http%3A%2F%2F127.0.0.1%3A1234%2Foauth2callback"
            ));
            assert!(url.contains("response_type=code"));
            assert!(url.contains(
                "code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
            ));
            assert!(url.contains("code_challenge_method=S256"));
            assert!(url.contains("state=state-1"));
            assert!(url.contains("access_type=offline"));
            assert!(url.contains("prompt=consent"));
            assert!(url.contains("scope="));
            assert!(url.contains("drive.file"));
            assert!(url.contains("userinfo.email"));
        }

        #[test]
        fn token_response_parses_expiry_and_refresh_merge() {
            let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
            let tokens = parse_token_response(
                r#"{"access_token":"ya29.a","expires_in":3600,"refresh_token":"1//x","token_type":"Bearer"}"#,
                now,
            )
            .unwrap();
            assert_eq!(tokens.access_token, "ya29.a");
            assert_eq!(tokens.refresh_token.as_deref(), Some("1//x"));
            assert_eq!(tokens.expires_at, now + chrono::Duration::seconds(3600));
            assert!(tokens.user_email.is_none());
            assert!(!tokens.is_expired_at(now));
            assert!(tokens.is_expired_at(now + chrono::Duration::seconds(3540)));

            let mut kept = tokens.clone();
            apply_refresh_response(
                &mut kept,
                r#"{"access_token":"ya29.b","expires_in":10}"#,
                now,
            )
            .unwrap();
            assert_eq!(kept.access_token, "ya29.b");
            assert_eq!(kept.refresh_token.as_deref(), Some("1//x"));
            assert_eq!(kept.expires_at, now + chrono::Duration::seconds(10));

            apply_refresh_response(
                &mut kept,
                r#"{"access_token":"ya29.c","expires_in":20,"refresh_token":"1//y"}"#,
                now,
            )
            .unwrap();
            assert_eq!(kept.refresh_token.as_deref(), Some("1//y"));

            assert!(parse_token_response("{}", now).is_err());
            assert!(parse_token_response(r#"{"access_token":"","expires_in":1}"#, now).is_err());
        }

        #[test]
        fn callback_returns_code_and_rejects_bad_state() {
            let ok = "GET /oauth2callback?code=abc%2F1&state=s1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
            assert_eq!(parse_oauth_callback(ok, "s1").unwrap(), "abc/1");
            let mismatch = "GET /oauth2callback?code=abc&state=nope HTTP/1.1\r\n\r\n";
            assert!(matches!(
                parse_oauth_callback(mismatch, "s1"),
                Err(GoogleOAuthError::AuthorizationFailed(_))
            ));
            let denied = "GET /oauth2callback?error=access_denied&state=s1 HTTP/1.1\r\n\r\n";
            assert!(matches!(
                parse_oauth_callback(denied, "s1"),
                Err(GoogleOAuthError::AuthorizationFailed(msg)) if msg == "access_denied"
            ));
        }
    }
}

mod client {
    use super::*;
    use chrono::{DateTime, Utc};
    use std::fs;
    use std::path::Path;
    use std::time::SystemTime;

    #[derive(Debug, Clone, PartialEq)]
    pub struct GoogleDriveItem {
        pub id: String,
        pub name: String,
        pub mime_type: String,
        pub size: i64,
        pub modified_time: Option<DateTime<Utc>>,
        pub md5_checksum: Option<String>,
    }

    impl GoogleDriveItem {
        pub fn is_folder(&self) -> bool {
            self.mime_type == "application/vnd.google-apps.folder"
        }
    }

    impl<'de> serde::Deserialize<'de> for GoogleDriveItem {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let value = serde_json::Value::deserialize(deserializer)?;
            parse_drive_item(&value).map_err(serde::de::Error::custom)
        }
    }

    pub fn parse_drive_item(value: &serde_json::Value) -> Result<GoogleDriveItem, String> {
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("missing id")?
            .to_string();
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("missing name")?
            .to_string();
        let mime_type = value
            .get("mimeType")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let size = match value.get("size") {
            Some(serde_json::Value::String(text)) => text.parse::<i64>().unwrap_or(0),
            Some(serde_json::Value::Number(number)) => number.as_i64().unwrap_or(0),
            _ => 0,
        };
        let modified_time = value
            .get("modifiedTime")
            .and_then(|v| v.as_str())
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let md5_checksum = value
            .get("md5Checksum")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(GoogleDriveItem {
            id,
            name,
            mime_type,
            size,
            modified_time,
            md5_checksum,
        })
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UploadStrategy {
        Multipart,
        Resumable,
    }

    pub fn upload_strategy(len: u64) -> UploadStrategy {
        if len < MULTIPART_LIMIT {
            UploadStrategy::Multipart
        } else {
            UploadStrategy::Resumable
        }
    }

    pub fn folder_search_query(name: &str, parent_id: &str) -> String {
        let name = drive_query_string(name);
        let parent = drive_query_string(parent_id);
        format!(
            "name = '{name}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false and '{parent}' in parents"
        )
    }

    pub fn list_children_query(folder_id: &str) -> String {
        let id = drive_query_string(folder_id);
        format!("'{id}' in parents and trashed = false")
    }

    /// Escape `\` then `'` for Drive `q` string literals.
    fn drive_query_string(value: &str) -> String {
        value.replace('\\', "\\\\").replace('\'', "\\'")
    }

    pub fn mime_type_for_path(path: &Path) -> &'static str {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "txt" | "swift" | "kt" | "ts" | "js" | "py" | "sh" | "rs" => "text/plain",
            "html" | "htm" => "text/html",
            "json" => "application/json",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "pdf" => "application/pdf",
            "zip" => "application/zip",
            "mp3" => "audio/mpeg",
            "mp4" => "video/mp4",
            "csv" => "text/csv",
            _ => "application/octet-stream",
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum GoogleDriveError {
        #[error("Google Drive API error ({status}): {message}")]
        Api { status: u16, message: String },
        #[error("Invalid response from Google Drive.")]
        InvalidResponse,
        #[error(transparent)]
        Oauth(#[from] super::oauth::GoogleOAuthError),
        #[error("{0}")]
        Message(String),
        #[error("sync interrupted")]
        Interrupted,
        #[error(transparent)]
        Io(#[from] std::io::Error),
    }

    pub struct GoogleDriveClient {
        http: reqwest::blocking::Client,
    }

    impl GoogleDriveClient {
        pub fn new() -> Result<Self, GoogleDriveError> {
            let http = http_client().map_err(GoogleDriveError::Message)?;
            Ok(Self { http })
        }

        pub fn get_or_create_folder(
            &self,
            token: &str,
            name: &str,
            parent_id: Option<&str>,
        ) -> Result<String, GoogleDriveError> {
            let parent = parent_id.unwrap_or("root");
            let query = folder_search_query(name, parent);
            let url = format!(
                "{DRIVE_FILES}?q={}&fields=files(id,name)&pageSize=10",
                percent_encode(&query)
            );
            let body = self.get_json(token, &url)?;
            if let Some(id) = body
                .get("files")
                .and_then(|v| v.as_array())
                .and_then(|files| files.first())
                .and_then(|file| file.get("id"))
                .and_then(|id| id.as_str())
            {
                return Ok(id.to_string());
            }
            let payload = serde_json::json!({
                "name": name,
                "mimeType": "application/vnd.google-apps.folder",
                "parents": [parent],
            });
            let created = self.post_json(token, DRIVE_FILES, payload)?;
            created
                .get("id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
                .ok_or(GoogleDriveError::InvalidResponse)
        }

        pub fn list_items(
            &self,
            token: &str,
            folder_id: &str,
        ) -> Result<Vec<GoogleDriveItem>, GoogleDriveError> {
            let mut items = Vec::new();
            let mut page_token: Option<String> = None;
            loop {
                let query = list_children_query(folder_id);
                let mut url = format!(
                    "{DRIVE_FILES}?q={}&fields=nextPageToken,files(id,name,mimeType,size,modifiedTime,md5Checksum)&pageSize=1000",
                    percent_encode(&query)
                );
                if let Some(token) = &page_token {
                    url.push_str("&pageToken=");
                    url.push_str(&percent_encode(token));
                }
                let body = self.get_json(token, &url)?;
                let page_items = body
                    .get("files")
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(Vec::new()));
                let parsed: Vec<GoogleDriveItem> =
                    serde_json::from_value(page_items).map_err(|_| GoogleDriveError::InvalidResponse)?;
                items.extend(parsed);
                page_token = body
                    .get("nextPageToken")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                if page_token.is_none() {
                    break;
                }
            }
            Ok(items)
        }

        pub fn upload_file(
            &self,
            token: &str,
            local: &Path,
            name: &str,
            parent_folder_id: &str,
            existing_file_id: Option<&str>,
        ) -> Result<GoogleDriveItem, GoogleDriveError> {
            let meta = fs::metadata(local)?;
            let len = meta.len();
            let mime = mime_type_for_path(local);
            let modified = rfc3339(meta.modified().ok());
            if upload_strategy(len) == UploadStrategy::Multipart {
                let bytes = fs::read(local)?;
                self.upload_multipart(
                    token,
                    &bytes,
                    name,
                    mime,
                    parent_folder_id,
                    &modified,
                    existing_file_id,
                )
            } else {
                self.upload_resumable(
                    token,
                    local,
                    len,
                    name,
                    mime,
                    parent_folder_id,
                    &modified,
                    existing_file_id,
                )
            }
        }

        pub fn delete_item(&self, token: &str, id: &str) -> Result<(), GoogleDriveError> {
            let url = format!("{DRIVE_FILES}/{}", percent_encode(id));
            let response = self
                .http
                .delete(url)
                .bearer_auth(token)
                .send()
                .map_err(|e| GoogleDriveError::Message(e.to_string()))?;
            let status = response.status().as_u16();
            if (200..300).contains(&status) || status == 404 {
                return Ok(());
            }
            let text = response.text().unwrap_or_default();
            Err(GoogleDriveError::Api {
                status,
                message: clip(&text, status),
            })
        }

        fn upload_multipart(
            &self,
            token: &str,
            file: &[u8],
            name: &str,
            mime: &str,
            parent_folder_id: &str,
            modified: &str,
            existing_file_id: Option<&str>,
        ) -> Result<GoogleDriveItem, GoogleDriveError> {
            let boundary = format!("Boundary-{}", uuid::Uuid::new_v4());
            let metadata = upload_metadata(name, modified, parent_folder_id, existing_file_id.is_some());
            let meta_bytes = serde_json::to_vec(&metadata).map_err(|_| GoogleDriveError::InvalidResponse)?;
            let body = multipart_body(&boundary, &meta_bytes, mime, file);
            let url = upload_endpoint(existing_file_id, "multipart");
            let method_patch = existing_file_id.is_some();
            let request = if method_patch {
                self.http.patch(url)
            } else {
                self.http.post(url)
            };
            let response = request
                .bearer_auth(token)
                .header(
                    "Content-Type",
                    format!("multipart/related; boundary={boundary}"),
                )
                .body(body)
                .send()
                .map_err(|e| GoogleDriveError::Message(e.to_string()))?;
            json_item(response)
        }

        fn upload_resumable(
            &self,
            token: &str,
            local: &Path,
            len: u64,
            name: &str,
            mime: &str,
            parent_folder_id: &str,
            modified: &str,
            existing_file_id: Option<&str>,
        ) -> Result<GoogleDriveItem, GoogleDriveError> {
            let metadata = upload_metadata(name, modified, parent_folder_id, existing_file_id.is_some());
            let url = upload_endpoint(existing_file_id, "resumable");
            let request = if existing_file_id.is_some() {
                self.http.patch(url)
            } else {
                self.http.post(url)
            };
            let response = request
                .bearer_auth(token)
                .header("Content-Type", "application/json; charset=UTF-8")
                .header("X-Upload-Content-Type", mime)
                .header("X-Upload-Content-Length", len.to_string())
                .json(&metadata)
                .send()
                .map_err(|e| GoogleDriveError::Message(e.to_string()))?;
            let status = response.status().as_u16();
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let Some(location) = location.filter(|_| status == 200) else {
                let text = response.text().unwrap_or_default();
                return Err(GoogleDriveError::Api {
                    status,
                    message: if text.is_empty() {
                        "Missing resumable upload location header.".into()
                    } else {
                        clip(&text, status)
                    },
                });
            };
            let file = fs::File::open(local)?;
            let put = self
                .http
                .put(location)
                .bearer_auth(token)
                .header("Content-Type", mime)
                .header("Content-Length", len.to_string())
                .body(file)
                .send()
                .map_err(|e| GoogleDriveError::Message(e.to_string()))?;
            json_item(put)
        }

        fn get_json(&self, token: &str, url: &str) -> Result<serde_json::Value, GoogleDriveError> {
            let response = self
                .http
                .get(url)
                .bearer_auth(token)
                .send()
                .map_err(|e| GoogleDriveError::Message(e.to_string()))?;
            ok_json(response)
        }

        fn post_json(
            &self,
            token: &str,
            url: &str,
            payload: serde_json::Value,
        ) -> Result<serde_json::Value, GoogleDriveError> {
            let response = self
                .http
                .post(url)
                .bearer_auth(token)
                .header("Content-Type", "application/json; charset=UTF-8")
                .json(&payload)
                .send()
                .map_err(|e| GoogleDriveError::Message(e.to_string()))?;
            ok_json(response)
        }
    }

    fn json_item(response: reqwest::blocking::Response) -> Result<GoogleDriveItem, GoogleDriveError> {
        let value = ok_json(response)?;
        parse_drive_item(&value).map_err(|_| GoogleDriveError::InvalidResponse)
    }

    fn ok_json(response: reqwest::blocking::Response) -> Result<serde_json::Value, GoogleDriveError> {
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|e| GoogleDriveError::Message(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(GoogleDriveError::Api {
                status,
                message: clip(&text, status),
            });
        }
        serde_json::from_str(&text).map_err(|_| GoogleDriveError::InvalidResponse)
    }

    fn upload_metadata(
        name: &str,
        modified: &str,
        parent_folder_id: &str,
        is_update: bool,
    ) -> serde_json::Value {
        let mut metadata = serde_json::json!({
            "name": name,
            "modifiedTime": modified,
        });
        if !is_update {
            metadata["parents"] = serde_json::json!([parent_folder_id]);
        }
        metadata
    }

    fn upload_endpoint(existing_file_id: Option<&str>, kind: &str) -> String {
        let fields = "id,name,mimeType,size,modifiedTime,md5Checksum";
        match existing_file_id {
            Some(id) => format!(
                "{DRIVE_UPLOAD}/{}?uploadType={kind}&fields={fields}",
                percent_encode(id)
            ),
            None => format!("{DRIVE_UPLOAD}?uploadType={kind}&fields={fields}"),
        }
    }

    fn multipart_body(boundary: &str, metadata: &[u8], mime: &str, file: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(format!("--{boundary}\r\n").into_bytes());
        body.extend(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
        body.extend(metadata);
        body.extend(format!("\r\n--{boundary}\r\n").into_bytes());
        body.extend(format!("Content-Type: {mime}\r\n\r\n").into_bytes());
        body.extend(file);
        body.extend(format!("\r\n--{boundary}--\r\n").into_bytes());
        body
    }

    fn rfc3339(modified: Option<SystemTime>) -> String {
        let dt = modified
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);
        dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn folder_query_escapes_quotes_and_strategy_splits_at_five_megabytes() {
            assert_eq!(
                folder_search_query("O'Brien", "root"),
                "name = 'O\\'Brien' and mimeType = 'application/vnd.google-apps.folder' and trashed = false and 'root' in parents"
            );
            assert_eq!(
                folder_search_query("a\\b'c", "root"),
                "name = 'a\\\\b\\'c' and mimeType = 'application/vnd.google-apps.folder' and trashed = false and 'root' in parents"
            );
            assert_eq!(upload_strategy(4_999_999), UploadStrategy::Multipart);
            assert_eq!(upload_strategy(5_000_000), UploadStrategy::Resumable);
            assert_eq!(mime_type_for_path(Path::new("a/b.PNG")), "image/png");
            assert_eq!(
                mime_type_for_path(Path::new("blob")),
                "application/octet-stream"
            );
        }

        #[test]
        fn drive_item_parses_string_size_and_rfc3339() {
            let value = serde_json::json!({
                "id": "f1",
                "name": "notes.txt",
                "mimeType": "text/plain",
                "size": "12",
                "modifiedTime": "2024-01-02T03:04:05.678Z",
                "md5Checksum": "abc"
            });
            let item = parse_drive_item(&value).unwrap();
            assert_eq!(item.size, 12);
            assert_eq!(
                item.modified_time.unwrap().to_rfc3339(),
                "2024-01-02T03:04:05.678+00:00"
            );
            assert!(!item.is_folder());
            let folder = serde_json::json!({
                "id": "d1",
                "name": "SyncDrop",
                "mimeType": "application/vnd.google-apps.folder"
            });
            assert!(parse_drive_item(&folder).unwrap().is_folder());
        }
    }
}

mod sync {
    use super::client::{GoogleDriveClient, GoogleDriveError, GoogleDriveItem};
    use super::oauth::GoogleOAuth;
    use super::MTIME_SKEW_MS;
    use crate::config::ConfigStore;
    use crate::progress::{State, SyncProgress, SyncRecord};
    use chrono::{DateTime, Utc};
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct LocalSnapshot {
        pub relative_path: String,
        pub absolute_path: PathBuf,
        pub size: i64,
        pub modified: DateTime<Utc>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RemoteSnapshot {
        pub id: String,
        pub relative_path: String,
        pub size: i64,
        pub modified: Option<DateTime<Utc>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum PlannedUpload {
        Create {
            relative_path: String,
            size: i64,
        },
        Update {
            relative_path: String,
            existing_id: String,
            size: i64,
        },
    }

    impl PlannedUpload {
        pub fn relative_path(&self) -> &str {
            match self {
                Self::Create { relative_path, .. } | Self::Update { relative_path, .. } => {
                    relative_path
                }
            }
        }

        pub fn size(&self) -> i64 {
            match self {
                Self::Create { size, .. } | Self::Update { size, .. } => *size,
            }
        }

        pub fn existing_id(&self) -> Option<&str> {
            match self {
                Self::Create { .. } => None,
                Self::Update { existing_id, .. } => Some(existing_id),
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DrivePlan {
        pub uploads: Vec<PlannedUpload>,
        pub deletes: Vec<RemoteSnapshot>,
        pub total_bytes: i64,
    }

    impl DrivePlan {
        pub fn file_count(&self) -> i64 {
            (self.uploads.len() + self.deletes.len()) as i64
        }
    }

    /// Upload when the file is new, the size differs, or the local mtime is more than 2s newer.
    /// A missing Drive `modifiedTime` is treated as ancient, so the file uploads.
    /// Mirror mode lists remote files that have no local path. Deletes are sorted by path.
    pub fn plan_drive_sync(
        local: &[LocalSnapshot],
        remote: &[RemoteSnapshot],
        mirror: bool,
    ) -> DrivePlan {
        let mut remote_by_path: HashMap<&str, &RemoteSnapshot> = HashMap::new();
        for item in remote {
            remote_by_path.insert(item.relative_path.as_str(), item);
        }
        let mut uploads = Vec::new();
        let mut total_bytes = 0i64;
        for file in local {
            match remote_by_path.get(file.relative_path.as_str()) {
                Some(existing) => {
                    let newer = match existing.modified {
                        Some(remote_mtime) => {
                            (file.modified - remote_mtime).num_milliseconds() > MTIME_SKEW_MS
                        }
                        None => true,
                    };
                    if file.size != existing.size || newer {
                        uploads.push(PlannedUpload::Update {
                            relative_path: file.relative_path.clone(),
                            existing_id: existing.id.clone(),
                            size: file.size,
                        });
                        total_bytes += file.size;
                    }
                }
                None => {
                    uploads.push(PlannedUpload::Create {
                        relative_path: file.relative_path.clone(),
                        size: file.size,
                    });
                    total_bytes += file.size;
                }
            }
        }
        let mut deletes = Vec::new();
        if mirror {
            let local_paths: HashSet<&str> = local.iter().map(|f| f.relative_path.as_str()).collect();
            for item in remote {
                if !local_paths.contains(item.relative_path.as_str()) {
                    deletes.push(item.clone());
                }
            }
            deletes.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        }
        DrivePlan {
            uploads,
            deletes,
            total_bytes,
        }
    }

    pub fn path_excluded(path: &str, excludes: &[String]) -> bool {
        for pattern in excludes {
            let p = pattern.trim();
            if p.is_empty() {
                continue;
            }
            if path == p
                || path.starts_with(&format!("{p}/"))
                || path.ends_with(&format!("/{p}"))
                || path.contains(&format!("/{p}/"))
            {
                return true;
            }
        }
        false
    }

    /// Regular files under `source`. Skips hidden names and excluded path segments.
    /// Symlinks are not followed.
    pub fn scan_local_files(source: &Path, excludes: &[String]) -> Vec<LocalSnapshot> {
        let mut out = Vec::new();
        let walker = walkdir::WalkDir::new(source)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                if entry.depth() == 0 {
                    return true;
                }
                let name = entry.file_name().to_string_lossy();
                if name.starts_with('.') {
                    return false;
                }
                match entry.path().strip_prefix(source) {
                    Ok(rel) => !path_excluded(&rel_string(rel), excludes),
                    Err(_) => false,
                }
            });
        for entry in walker.flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(source) else {
                continue;
            };
            let relative_path = rel_string(rel);
            if path_excluded(&relative_path, excludes) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .map(DateTime::<Utc>::from)
                .unwrap_or_else(Utc::now);
            let size = i64::try_from(meta.len()).unwrap_or(i64::MAX);
            out.push(LocalSnapshot {
                relative_path,
                absolute_path: entry.path().to_path_buf(),
                size,
                modified,
            });
        }
        out
    }

    fn rel_string(path: &Path) -> String {
        path.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    pub struct GoogleDriveSync {
        oauth: GoogleOAuth,
        client: GoogleDriveClient,
        progress: SyncProgress,
        cancel: Arc<AtomicBool>,
    }

    impl GoogleDriveSync {
        pub fn new(oauth: GoogleOAuth) -> Result<Self, GoogleDriveError> {
            Ok(Self {
                oauth,
                client: GoogleDriveClient::new()?,
                progress: SyncProgress::new(),
                cancel: Arc::new(AtomicBool::new(false)),
            })
        }

        pub fn oauth(&mut self) -> &mut GoogleOAuth {
            &mut self.oauth
        }

        pub fn progress(&self) -> &SyncProgress {
            &self.progress
        }

        /// Share with another thread; store `true` to stop the in-flight [`GoogleDriveSync::run`].
        pub fn cancel_flag(&self) -> Arc<AtomicBool> {
            Arc::clone(&self.cancel)
        }

        pub fn cancel(&self) {
            self.cancel.store(true, Ordering::Relaxed);
        }

        /// Blocking Drive sync of `profile`.
        ///
        /// `on_progress` is `(files_done, files_total, bytes_done, bytes_total, current_path)`.
        pub fn run(
            &mut self,
            store: &mut ConfigStore,
            profile: &crate::profile::SyncProfile,
            on_progress: &dyn Fn(i64, i64, i64, i64, &str),
        ) -> Result<(), GoogleDriveError> {
            self.cancel.store(false, Ordering::Relaxed);
            let source = profile.expanded_source_path();
            if source.is_empty() {
                return Err(self.precondition("Source path not configured"));
            }
            if !self.oauth.is_signed_in() {
                return Err(self.precondition(
                    "Not signed in to Google Drive. Please sign in via Settings.",
                ));
            }
            let source_path = PathBuf::from(&source);
            if !source_path.is_dir() {
                return Err(self.precondition(format!("Source folder does not exist: {source}")));
            }
            self.progress = SyncProgress {
                state: State::Running,
                start_time: Some(Utc::now()),
                ..SyncProgress::new()
            };
            match self.run_inner(profile, &source_path, store, on_progress) {
                Ok(()) => Ok(()),
                Err(err) => {
                    if matches!(self.progress.state, State::Interrupted)
                        || matches!(err, GoogleDriveError::Interrupted)
                    {
                        self.progress.end_time = Some(Utc::now());
                        self.progress.state = State::Interrupted;
                        return Err(GoogleDriveError::Interrupted);
                    }
                    if !matches!(self.progress.state, State::Error(_)) {
                        self.progress.state =
                            State::Error(format!("Google Drive error: {err}"));
                    }
                    Err(err)
                }
            }
        }

        fn run_inner(
            &mut self,
            profile: &crate::profile::SyncProfile,
            source: &Path,
            store: &mut ConfigStore,
            on_progress: &dyn Fn(i64, i64, i64, i64, &str),
        ) -> Result<(), GoogleDriveError> {
            let root_name = {
                let trimmed = self.oauth.target_folder_name().trim();
                if trimmed.is_empty() {
                    "SyncDrop".to_string()
                } else {
                    trimmed.to_string()
                }
            };
            let profile_name = profile.name.clone();
            let mirror = profile.mirror_mode;
            let excludes = profile.excludes.clone();

            let token = self.token()?;
            let root_id = self
                .client
                .get_or_create_folder(&token, &root_name, Some("root"))?;
            let token = self.token()?;
            let profile_id =
                self.client
                    .get_or_create_folder(&token, &profile_name, Some(&root_id))?;
            self.check_cancel()?;

            let local_files = scan_local_files(source, &excludes);
            let remote_files = self.scan_remote(&profile_id, "")?;
            self.check_cancel()?;

            let plan = plan_drive_sync(&local_files, &remote_files, mirror);
            self.progress.files_total = plan.file_count();
            self.progress.bytes_total = plan.total_bytes;
            self.progress.files_done = 0;
            self.report(on_progress);

            if plan.uploads.is_empty() && plan.deletes.is_empty() {
                return self.complete(store, &profile_name, &profile.id.to_string(), on_progress);
            }

            let by_path: HashMap<&str, &LocalSnapshot> = local_files
                .iter()
                .map(|file| (file.relative_path.as_str(), file))
                .collect();
            let mut cache = HashMap::new();
            cache.insert(String::new(), profile_id.clone());

            for upload in &plan.uploads {
                self.check_cancel()?;
                let rel = upload.relative_path();
                self.progress.current_file = rel.to_string();
                self.report(on_progress);
                let parent = parent_rel(rel);
                let parent_id = self.ensure_folder(parent, &profile_id, &mut cache)?;
                let Some(local) = by_path.get(rel) else {
                    continue;
                };
                let name = file_name(rel).to_string();
                let token = self.token()?;
                self.client.upload_file(
                    &token,
                    &local.absolute_path,
                    &name,
                    &parent_id,
                    upload.existing_id(),
                )?;
                self.progress.files_done += 1;
                self.progress.bytes_done += upload.size();
                self.report(on_progress);
            }

            for item in &plan.deletes {
                self.check_cancel()?;
                let name = file_name(&item.relative_path);
                self.progress.current_file = format!("Removing: {name}");
                self.report(on_progress);
                let token = self.token()?;
                self.client.delete_item(&token, &item.id)?;
                self.progress.files_done += 1;
                self.report(on_progress);
            }

            self.complete(store, &profile_name, &profile.id.to_string(), on_progress)
        }

        fn scan_remote(
            &mut self,
            folder_id: &str,
            prefix: &str,
        ) -> Result<Vec<RemoteSnapshot>, GoogleDriveError> {
            let token = self.token()?;
            let items = self.client.list_items(&token, folder_id)?;
            let mut out = Vec::new();
            for item in items {
                let path = if prefix.is_empty() {
                    item.name.clone()
                } else {
                    format!("{prefix}/{}", item.name)
                };
                if item.is_folder() {
                    // ponytail: recursive listing. Fine for normal trees; a work queue is the upgrade for pathological depth.
                    out.extend(self.scan_remote(&item.id, &path)?);
                } else {
                    out.push(remote_snapshot(item, path));
                }
            }
            Ok(out)
        }

        fn ensure_folder(
            &mut self,
            rel: &str,
            profile_folder_id: &str,
            cache: &mut HashMap<String, String>,
        ) -> Result<String, GoogleDriveError> {
            if rel.is_empty() {
                return Ok(profile_folder_id.to_string());
            }
            if let Some(id) = cache.get(rel) {
                return Ok(id.clone());
            }
            let mut current = profile_folder_id.to_string();
            let mut current_path = String::new();
            for component in rel.split('/').filter(|s| !s.is_empty()) {
                if !current_path.is_empty() {
                    current_path.push('/');
                }
                current_path.push_str(component);
                if let Some(id) = cache.get(&current_path) {
                    current = id.clone();
                    continue;
                }
                let token = self.token()?;
                let id = self
                    .client
                    .get_or_create_folder(&token, component, Some(&current))?;
                cache.insert(current_path.clone(), id.clone());
                current = id;
            }
            Ok(current)
        }

        fn token(&mut self) -> Result<String, GoogleDriveError> {
            self.oauth.get_valid_access_token().map_err(Into::into)
        }

        fn check_cancel(&mut self) -> Result<(), GoogleDriveError> {
            if self.cancel.load(Ordering::Relaxed) {
                self.progress.end_time = Some(Utc::now());
                self.progress.state = State::Interrupted;
                return Err(GoogleDriveError::Interrupted);
            }
            Ok(())
        }

        fn report(&self, on_progress: &dyn Fn(i64, i64, i64, i64, &str)) {
            on_progress(
                self.progress.files_done,
                self.progress.files_total,
                self.progress.bytes_done,
                self.progress.bytes_total,
                &self.progress.current_file,
            );
        }

        fn complete(
            &mut self,
            store: &mut ConfigStore,
            profile_name: &str,
            sync_profile_id: &str,
            on_progress: &dyn Fn(i64, i64, i64, i64, &str),
        ) -> Result<(), GoogleDriveError> {
            self.progress.end_time = Some(Utc::now());
            self.progress.state = State::Done;
            self.report(on_progress);
            let mut record = SyncRecord::new(
                Utc::now(),
                self.progress.files_done,
                self.progress.bytes_done,
                self.progress.duration().unwrap_or(0.0),
                true,
            );
            record.profile_name = profile_name.to_string();
            record.profile_id = sync_profile_id.to_string();
            store
                .append_sync_record(record)
                .map_err(|err| GoogleDriveError::Message(err.to_string()))?;
            Ok(())
        }

        fn precondition(&mut self, message: impl Into<String>) -> GoogleDriveError {
            let message = message.into();
            self.progress.state = State::Error(message.clone());
            GoogleDriveError::Message(message)
        }
    }

    fn remote_snapshot(item: GoogleDriveItem, path: String) -> RemoteSnapshot {
        RemoteSnapshot {
            id: item.id,
            relative_path: path,
            size: item.size,
            modified: item.modified_time,
        }
    }

    fn parent_rel(rel: &str) -> &str {
        match rel.rfind('/') {
            Some(index) => &rel[..index],
            None => "",
        }
    }

    fn file_name(rel: &str) -> &str {
        rel.rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(rel)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn local(path: &str, size: i64, modified: DateTime<Utc>) -> LocalSnapshot {
            LocalSnapshot {
                relative_path: path.to_string(),
                absolute_path: PathBuf::from(path),
                size,
                modified,
            }
        }

        fn remote(
            id: &str,
            path: &str,
            size: i64,
            modified: Option<DateTime<Utc>>,
        ) -> RemoteSnapshot {
            RemoteSnapshot {
                id: id.to_string(),
                relative_path: path.to_string(),
                size,
                modified,
            }
        }

        #[test]
        fn upload_diff_queues_creates_updates_and_mirror_deletes() {
            let t0 = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
            let local_files = vec![
                local("new.txt", 10, t0),
                local("same.txt", 4, t0 + chrono::Duration::seconds(2)),
                local("newer.txt", 4, t0 + chrono::Duration::milliseconds(2001)),
                local("resized.txt", 9, t0 - chrono::Duration::seconds(30)),
                local("unknown-mtime.txt", 3, t0),
            ];
            let remote_files = vec![
                remote("id-same", "same.txt", 4, Some(t0)),
                remote("id-newer", "newer.txt", 4, Some(t0)),
                remote("id-resized", "resized.txt", 8, Some(t0)),
                remote("id-ancient", "unknown-mtime.txt", 3, None),
                remote("id-gone", "only-remote.txt", 1, Some(t0)),
                remote("id-also", "nested/old.txt", 2, Some(t0)),
            ];

            let mirrored = plan_drive_sync(&local_files, &remote_files, true);
            assert_eq!(mirrored.uploads.len(), 4);
            assert!(matches!(
                &mirrored.uploads[0],
                PlannedUpload::Create { relative_path, size } if relative_path == "new.txt" && *size == 10
            ));
            assert!(mirrored
                .uploads
                .iter()
                .all(|u| u.relative_path() != "same.txt"));
            assert!(matches!(
                mirrored.uploads.iter().find(|u| u.relative_path() == "newer.txt"),
                Some(PlannedUpload::Update { existing_id, .. }) if existing_id == "id-newer"
            ));
            assert!(matches!(
                mirrored.uploads.iter().find(|u| u.relative_path() == "resized.txt"),
                Some(PlannedUpload::Update { existing_id, size, .. }) if existing_id == "id-resized" && *size == 9
            ));
            assert!(mirrored
                .uploads
                .iter()
                .any(|u| u.relative_path() == "unknown-mtime.txt"));
            assert_eq!(mirrored.total_bytes, 10 + 4 + 9 + 3);
            assert_eq!(
                mirrored
                    .deletes
                    .iter()
                    .map(|d| d.relative_path.as_str())
                    .collect::<Vec<_>>(),
                vec!["nested/old.txt", "only-remote.txt"]
            );
            assert_eq!(mirrored.file_count(), 6);

            let keep = plan_drive_sync(&local_files, &remote_files, false);
            assert!(keep.deletes.is_empty());
            assert_eq!(keep.uploads.len(), mirrored.uploads.len());
        }

        #[test]
        fn scan_skips_hidden_files_and_excluded_dirs() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("keep.txt"), b"hi").unwrap();
            std::fs::create_dir(dir.path().join("nested")).unwrap();
            std::fs::write(dir.path().join("nested").join("child.txt"), b"abcd").unwrap();
            std::fs::create_dir(dir.path().join("node_modules")).unwrap();
            std::fs::write(dir.path().join("node_modules").join("pkg.js"), b"x").unwrap();
            std::fs::write(dir.path().join(".secret"), b"no").unwrap();

            let files = scan_local_files(dir.path(), &["node_modules".to_string()]);
            let mut paths: Vec<_> = files.iter().map(|f| f.relative_path.as_str()).collect();
            paths.sort();
            assert_eq!(paths, vec!["keep.txt", "nested/child.txt"]);
            let keep = files.iter().find(|f| f.relative_path == "keep.txt").unwrap();
            assert_eq!(keep.size, 2);
            assert!(keep.absolute_path.ends_with("keep.txt"));
        }
    }
}

pub use client::{
    folder_search_query, list_children_query, mime_type_for_path, parse_drive_item,
    upload_strategy, GoogleDriveClient, GoogleDriveError, GoogleDriveItem, UploadStrategy,
};
pub use oauth::{
    apply_refresh_response, build_auth_url, generate_pkce, oauth_redirect_uri, open_system_browser,
    parse_oauth_callback, parse_token_response, pkce_challenge, GoogleOAuth, GoogleOAuthError,
    GoogleTokens, OAUTH_CALLBACK_PATH,
};
pub use sync::{
    path_excluded, plan_drive_sync, scan_local_files, DrivePlan, GoogleDriveSync, LocalSnapshot,
    PlannedUpload, RemoteSnapshot,
};
