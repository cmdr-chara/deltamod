use crate::state::AppState;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use deltamod_credentials_adapter::{CredentialKind, Secret};
use deltamod_network_runtime::{Nexus, NexusUser};
use reqwest::{blocking::Client, redirect::Policy, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{ErrorKind, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use url::Url;
use uuid::Uuid;

pub const CALLBACK_HOST: &str = "127.0.0.1";
pub const CALLBACK_PORT: u16 = 52817;
pub const CALLBACK_PATH: &str = "/callback";
pub const REDIRECT_URI: &str = "http://127.0.0.1:52817/callback";
const CALLBACK_AUTHORITY: &str = "127.0.0.1:52817";
const AUTHORIZATION_ENDPOINT: &str = "https://users.nexusmods.com/oauth/authorize";
const TOKEN_ENDPOINT: &str = "https://users.nexusmods.com/oauth/token";
const MAX_CALLBACK_BYTES: usize = 8192;
const MAX_TOKEN_BYTES: u64 = 32 * 1024;
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct OAuthFailure {
    pub code: &'static str,
    pub message: &'static str,
    pub status: Option<u16>,
}

impl OAuthFailure {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            status: None,
        }
    }

    fn with_status(mut self, status: StatusCode) -> Self {
        self.status = Some(status.as_u16());
        self
    }

    pub fn response(&self) -> Value {
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(status) = self.status {
            error["status"] = json!(status);
        }
        json!({"ok": false, "error": error})
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    schema_version: u8,
    access_token: String,
    refresh_token: String,
    token_type: String,
    issued_at: u64,
    expires_at: u64,
    scope: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    token_type: String,
    expires_in: u64,
    #[serde(default)]
    scope: String,
}

fn package_value(name: &str) -> Option<String> {
    serde_json::from_str::<Value>(include_str!("../../../package.json"))
        .ok()?
        .get(name)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub fn configured_client_id() -> Option<String> {
    std::env::var("DELTAMOD_NEXUS_OAUTH_CLIENT_ID")
        .ok()
        .or_else(|| package_value("nexusOAuthClientId"))
        .map(|value| value.trim().to_owned())
        .filter(|value| {
            (1..=200).contains(&value.len())
                && value
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
                })
        })
}

fn configured_scope() -> String {
    std::env::var("DELTAMOD_NEXUS_OAUTH_SCOPE")
        .ok()
        .or_else(|| package_value("nexusOAuthScope"))
        .map(|value| value.trim().to_owned())
        .filter(|value| {
            value.len() <= 256
                && value.split(' ').all(|part| {
                    !part.is_empty()
                        && part.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'.' | b'_' | b':' | b'-')
                        })
                })
        })
        .unwrap_or_default()
}

fn now_millis() -> Result<u64, OAuthFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .map_err(|_| OAuthFailure::new("NEXUS_OAUTH_CLOCK_INVALID", "The system clock is invalid."))
}

fn valid_token(value: &str) -> bool {
    (20..=8192).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_scope(value: &str) -> bool {
    value.is_empty()
        || (value.len() <= 256
            && value.split(' ').all(|part| {
                !part.is_empty()
                    && part.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
                    })
            }))
}

fn validate_tokens(tokens: OAuthTokens) -> Result<OAuthTokens, OAuthFailure> {
    if tokens.schema_version != 1
        || !valid_token(&tokens.access_token)
        || !valid_token(&tokens.refresh_token)
        || tokens.token_type != "Bearer"
        || tokens.expires_at <= tokens.issued_at
        || !valid_scope(&tokens.scope)
    {
        return Err(OAuthFailure::new(
            "NEXUS_CREDENTIAL_INVALID",
            "The saved Nexus Mods authorization is invalid.",
        ));
    }
    Ok(tokens)
}

fn tokens_from_response(
    response: TokenResponse,
    fallback_refresh_token: Option<&str>,
) -> Result<OAuthTokens, OAuthFailure> {
    let issued_at = now_millis()?;
    let refresh_token = response
        .refresh_token
        .or_else(|| fallback_refresh_token.map(str::to_owned))
        .unwrap_or_default();
    if response.expires_in == 0 || response.expires_in > 365 * 24 * 60 * 60 {
        return Err(OAuthFailure::new(
            "NEXUS_OAUTH_INVALID_TOKEN_RESPONSE",
            "Nexus Mods returned an invalid OAuth token response.",
        ));
    }
    validate_tokens(OAuthTokens {
        schema_version: 1,
        access_token: response.access_token,
        refresh_token,
        token_type: if response.token_type.eq_ignore_ascii_case("bearer") {
            "Bearer".to_owned()
        } else {
            response.token_type
        },
        issued_at,
        expires_at: issued_at.saturating_add(response.expires_in.saturating_mul(1000)),
        scope: response.scope,
    })
    .map_err(|_| {
        OAuthFailure::new(
            "NEXUS_OAUTH_INVALID_TOKEN_RESPONSE",
            "Nexus Mods returned an invalid OAuth token response.",
        )
    })
}

fn token_request(
    parameters: &[(&str, &str)],
    refreshing: bool,
    fallback_refresh_token: Option<&str>,
) -> Result<OAuthTokens, OAuthFailure> {
    let client = Client::builder()
        .timeout(TOKEN_TIMEOUT)
        .redirect(Policy::none())
        .user_agent("Deltamod Community OAuth/2.0.3")
        .build()
        .map_err(|_| {
            OAuthFailure::new(
                "NEXUS_OAUTH_CONNECTION_FAILED",
                "Could not initialize the Nexus Mods authorization request.",
            )
        })?;
    let response = client
        .post(TOKEN_ENDPOINT)
        .header("accept", "application/json")
        .header("cache-control", "no-store")
        .form(parameters)
        .send()
        .map_err(|_| {
            OAuthFailure::new(
                "NEXUS_OAUTH_CONNECTION_FAILED",
                "Could not contact the Nexus Mods authorization service.",
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let reauth = refreshing && status.is_client_error();
        return Err(OAuthFailure::new(
            if reauth {
                "NEXUS_OAUTH_REAUTH_REQUIRED"
            } else {
                "NEXUS_OAUTH_TOKEN_FAILED"
            },
            if reauth {
                "The Nexus Mods authorization has expired or was revoked. Sign in again."
            } else {
                "Nexus Mods rejected the OAuth token exchange."
            },
        )
        .with_status(status));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_TOKEN_BYTES)
    {
        return Err(OAuthFailure::new(
            "NEXUS_OAUTH_INVALID_TOKEN_RESPONSE",
            "Nexus Mods returned an oversized OAuth response.",
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_TOKEN_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            OAuthFailure::new(
                "NEXUS_OAUTH_INVALID_TOKEN_RESPONSE",
                "Nexus Mods returned an unreadable OAuth response.",
            )
        })?;
    if bytes.len() as u64 > MAX_TOKEN_BYTES {
        return Err(OAuthFailure::new(
            "NEXUS_OAUTH_INVALID_TOKEN_RESPONSE",
            "Nexus Mods returned an oversized OAuth response.",
        ));
    }
    let payload: TokenResponse = serde_json::from_slice(&bytes).map_err(|_| {
        OAuthFailure::new(
            "NEXUS_OAUTH_INVALID_TOKEN_RESPONSE",
            "Nexus Mods returned malformed OAuth token data.",
        )
    })?;
    tokens_from_response(payload, fallback_refresh_token)
}

fn store_tokens(state: &AppState, tokens: &OAuthTokens) -> Result<(), OAuthFailure> {
    let store = state.credentials.as_ref().ok_or_else(|| {
        OAuthFailure::new(
            "SECURE_STORAGE_UNAVAILABLE",
            "Secure credential storage is unavailable on this system.",
        )
    })?;
    let encoded = serde_json::to_string(tokens).map_err(|_| {
        OAuthFailure::new(
            "NEXUS_CREDENTIAL_INVALID",
            "The Nexus Mods authorization could not be saved.",
        )
    })?;
    let secret = Secret::new(encoded).map_err(|_| {
        OAuthFailure::new(
            "NEXUS_CREDENTIAL_INVALID",
            "The Nexus Mods authorization could not be saved.",
        )
    })?;
    store
        .store(CredentialKind::NexusOAuthTokens, secret)
        .map_err(|_| {
            OAuthFailure::new(
                "SECURE_STORAGE_UNAVAILABLE",
                "The Nexus Mods authorization could not be saved securely.",
            )
        })?;
    let _ = store.clear(CredentialKind::NexusLegacySsoKey);
    Ok(())
}

fn load_tokens(state: &AppState) -> Result<Option<OAuthTokens>, OAuthFailure> {
    let Some(store) = state.credentials.as_ref() else {
        return Ok(None);
    };
    if store
        .load(CredentialKind::NexusLegacySsoKey)
        .ok()
        .flatten()
        .is_some()
    {
        let _ = store.clear(CredentialKind::NexusLegacySsoKey);
    }
    let Some(secret) = store.load(CredentialKind::NexusOAuthTokens).map_err(|_| {
        OAuthFailure::new(
            "SECURE_STORAGE_UNAVAILABLE",
            "The saved Nexus Mods authorization could not be read.",
        )
    })?
    else {
        return Ok(None);
    };
    let parsed = serde_json::from_str(secret.expose())
        .ok()
        .and_then(|tokens| validate_tokens(tokens).ok());
    if parsed.is_none() {
        let _ = store.clear(CredentialKind::NexusOAuthTokens);
        return Err(OAuthFailure::new(
            "NEXUS_CREDENTIAL_INVALID",
            "The saved Nexus Mods authorization is invalid. Sign in again.",
        ));
    }
    Ok(parsed)
}

pub fn has_tokens(state: &AppState) -> bool {
    state
        .credentials
        .as_ref()
        .and_then(|store| store.load(CredentialKind::NexusOAuthTokens).ok())
        .flatten()
        .is_some()
}

pub fn clear_tokens(state: &AppState) -> Result<(), OAuthFailure> {
    cancel(state);
    let _mutation = state.nexus_oauth_refresh.lock().map_err(|_| {
        OAuthFailure::new(
            "SECURE_STORAGE_UNAVAILABLE",
            "The saved Nexus Mods authorization could not be removed.",
        )
    })?;
    let store = state.credentials.as_ref().ok_or_else(|| {
        OAuthFailure::new(
            "SECURE_STORAGE_UNAVAILABLE",
            "Secure credential storage is unavailable on this system.",
        )
    })?;
    store.clear(CredentialKind::NexusOAuthTokens).map_err(|_| {
        OAuthFailure::new(
            "SECURE_STORAGE_UNAVAILABLE",
            "The saved Nexus Mods authorization could not be removed.",
        )
    })?;
    store
        .clear(CredentialKind::NexusLegacySsoKey)
        .map_err(|_| {
            OAuthFailure::new(
                "SECURE_STORAGE_UNAVAILABLE",
                "The legacy Nexus Mods authorization could not be removed.",
            )
        })?;
    Ok(())
}

pub fn access_token(state: &AppState) -> Result<Option<String>, OAuthFailure> {
    let Some(tokens) = load_tokens(state)? else {
        return Ok(None);
    };
    let now = now_millis()?;
    if tokens.expires_at > now.saturating_add(60_000) {
        return Ok(Some(tokens.access_token));
    }

    let _refresh = state.nexus_oauth_refresh.lock().map_err(|_| {
        OAuthFailure::new(
            "NEXUS_OAUTH_REFRESH_FAILED",
            "The Nexus Mods authorization could not be refreshed.",
        )
    })?;
    let Some(tokens) = load_tokens(state)? else {
        return Ok(None);
    };
    if tokens.expires_at > now_millis()?.saturating_add(60_000) {
        return Ok(Some(tokens.access_token));
    }
    let client_id = configured_client_id().ok_or_else(|| {
        OAuthFailure::new(
            "NEXUS_SSO_NOT_REGISTERED",
            "Nexus Mods sign-in is waiting for the OAuth client ID issued during registration.",
        )
    })?;
    let parameters = [
        ("grant_type", "refresh_token"),
        ("refresh_token", tokens.refresh_token.as_str()),
        ("client_id", client_id.as_str()),
    ];
    match token_request(&parameters, true, Some(&tokens.refresh_token)) {
        Ok(refreshed) => {
            store_tokens(state, &refreshed)?;
            Ok(Some(refreshed.access_token))
        }
        Err(error) => {
            if error.code == "NEXUS_OAUTH_REAUTH_REQUIRED" {
                if let Some(store) = state.credentials.as_ref() {
                    let _ = store.clear(CredentialKind::NexusOAuthTokens);
                }
            }
            Err(error)
        }
    }
}

pub fn validate_access_token(
    state: &AppState,
    access_token: String,
) -> Result<NexusUser, OAuthFailure> {
    state
        .network_runtime
        .lock()
        .map_err(|_| {
            OAuthFailure::new(
                "NEXUS_STATUS_FAILED",
                "Nexus Mods status could not be loaded.",
            )
        })?
        .block_on(
            Nexus {
                client: &state.network,
                access_token: Some(access_token),
            }
            .validate(),
        )
        .map_err(|_| {
            OAuthFailure::new(
                "NEXUS_AUTH_FAILED",
                "Nexus Mods rejected the authorization or this operation.",
            )
        })
}

fn verifier_and_challenge() -> (String, String) {
    let verifier = format!("{}-{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn random_state() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn write_callback_response(stream: &mut TcpStream, status: &str, title: &str, message: &str) {
    let body = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>{title}</title><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    );
    let headers = format!(
        "HTTP/1.1 {status}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'; base-uri 'none'\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(headers.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn read_callback_request(stream: &mut TcpStream) -> Result<String, OAuthFailure> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while request.len() <= MAX_CALLBACK_BYTES {
        let count = stream.read(&mut buffer).map_err(|_| {
            OAuthFailure::new(
                "NEXUS_OAUTH_CALLBACK_FAILED",
                "The local Nexus Mods callback could not be read.",
            )
        })?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    if request.len() > MAX_CALLBACK_BYTES || !request.windows(4).any(|window| window == b"\r\n\r\n")
    {
        return Err(OAuthFailure::new(
            "NEXUS_OAUTH_CALLBACK_FAILED",
            "The local Nexus Mods callback was malformed.",
        ));
    }
    String::from_utf8(request).map_err(|_| {
        OAuthFailure::new(
            "NEXUS_OAUTH_CALLBACK_FAILED",
            "The local Nexus Mods callback was malformed.",
        )
    })
}

enum CallbackOutcome {
    Continue,
    Code(String),
    Rejected,
}

fn handle_callback(stream: &mut TcpStream, state: &str) -> CallbackOutcome {
    let Ok(request) = read_callback_request(stream) else {
        write_callback_response(
            stream,
            "400 Bad Request",
            "Request rejected",
            "The OAuth callback was malformed.",
        );
        return CallbackOutcome::Continue;
    };
    let mut lines = request.split("\r\n");
    let Some(request_line) = lines.next() else {
        return CallbackOutcome::Continue;
    };
    let parts: Vec<_> = request_line.split(' ').collect();
    let host = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("host").then(|| value.trim())
    });
    if parts.len() != 3 || parts[0] != "GET" || host != Some(CALLBACK_AUTHORITY) {
        write_callback_response(
            stream,
            "400 Bad Request",
            "Request rejected",
            "Only the local OAuth callback is accepted.",
        );
        return CallbackOutcome::Continue;
    }
    let Ok(url) = Url::parse(&format!("http://{CALLBACK_AUTHORITY}{}", parts[1])) else {
        write_callback_response(
            stream,
            "400 Bad Request",
            "Request rejected",
            "The OAuth callback was malformed.",
        );
        return CallbackOutcome::Continue;
    };
    if url.path() != CALLBACK_PATH {
        write_callback_response(
            stream,
            "404 Not Found",
            "Not found",
            "This listener only accepts the OAuth callback.",
        );
        return CallbackOutcome::Continue;
    }
    let states: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .collect();
    if states.len() != 1 || !constant_time_equal(&states[0], state) {
        write_callback_response(
            stream,
            "400 Bad Request",
            "Request rejected",
            "The OAuth callback could not be verified.",
        );
        return CallbackOutcome::Continue;
    }
    let errors: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key == "error")
        .collect();
    if !errors.is_empty() {
        write_callback_response(
            stream,
            "400 Bad Request",
            "Authorization declined",
            "Return to Deltamod Community to try again.",
        );
        return CallbackOutcome::Rejected;
    }
    let codes: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .collect();
    if codes.len() != 1
        || !(8..=4096).contains(&codes[0].len())
        || !codes[0].bytes().all(|byte| byte.is_ascii_graphic())
    {
        write_callback_response(
            stream,
            "400 Bad Request",
            "Request rejected",
            "Nexus Mods did not provide a valid authorization code.",
        );
        return CallbackOutcome::Continue;
    }
    write_callback_response(
        stream,
        "200 OK",
        "Authorization received",
        "You can close this tab and return to Deltamod Community.",
    );
    CallbackOutcome::Code(codes[0].clone())
}

fn wait_for_code(
    listener: TcpListener,
    expected_state: &str,
    cancelled: &AtomicBool,
) -> Result<String, OAuthFailure> {
    listener.set_nonblocking(true).map_err(|_| {
        OAuthFailure::new(
            "NEXUS_OAUTH_CALLBACK_FAILED",
            "The local Nexus Mods callback listener failed.",
        )
    })?;
    let started = Instant::now();
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(OAuthFailure::new(
                "NEXUS_SSO_CANCELLED",
                "Nexus Mods sign-in was cancelled.",
            ));
        }
        if started.elapsed() >= LOGIN_TIMEOUT {
            return Err(OAuthFailure::new(
                "NEXUS_SSO_TIMEOUT",
                "Nexus Mods sign-in timed out. Start it again when you are ready.",
            ));
        }
        match listener.accept() {
            Ok((mut stream, peer)) if peer.ip() == Ipv4Addr::LOCALHOST => {
                match handle_callback(&mut stream, expected_state) {
                    CallbackOutcome::Continue => {}
                    CallbackOutcome::Code(code) => return Ok(code),
                    CallbackOutcome::Rejected => {
                        return Err(OAuthFailure::new(
                            "NEXUS_OAUTH_REJECTED",
                            "Nexus Mods authorization was declined or cancelled.",
                        ));
                    }
                }
            }
            Ok((mut stream, _)) => write_callback_response(
                &mut stream,
                "403 Forbidden",
                "Request rejected",
                "This callback is only available locally.",
            ),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                return Err(OAuthFailure::new(
                    "NEXUS_OAUTH_CALLBACK_FAILED",
                    "The local Nexus Mods callback listener failed.",
                ));
            }
        }
    }
}

fn run_login(
    app: &AppHandle,
    state: &AppState,
    cancel: Arc<AtomicBool>,
) -> Result<Value, OAuthFailure> {
    let client_id = configured_client_id().ok_or_else(|| {
        OAuthFailure::new(
            "NEXUS_SSO_NOT_REGISTERED",
            "Nexus Mods sign-in is waiting for the OAuth client ID issued during registration.",
        )
    })?;
    if state.credentials.is_none() {
        return Err(OAuthFailure::new(
            "SECURE_STORAGE_UNAVAILABLE",
            "Secure credential storage is unavailable. Nexus Mods sign-in cannot be saved.",
        ));
    }
    let callback_ip = CALLBACK_HOST.parse::<Ipv4Addr>().map_err(|_| {
        OAuthFailure::new(
            "NEXUS_OAUTH_CONFIGURATION_INVALID",
            "The fixed Nexus Mods callback address is invalid.",
        )
    })?;
    let listener = TcpListener::bind(SocketAddrV4::new(callback_ip, CALLBACK_PORT))
        .map_err(|_| {
            OAuthFailure::new(
                "NEXUS_OAUTH_CALLBACK_UNAVAILABLE",
                "The fixed Nexus Mods callback on 127.0.0.1:52817 is unavailable. Close the app using that port and try again.",
            )
        })?;
    let (verifier, challenge) = verifier_and_challenge();
    let expected_state = random_state();
    let scope = configured_scope();
    let mut authorization_url = Url::parse(AUTHORIZATION_ENDPOINT).map_err(|_| {
        OAuthFailure::new(
            "NEXUS_OAUTH_CONFIGURATION_INVALID",
            "The Nexus Mods authorization endpoint is invalid.",
        )
    })?;
    authorization_url
        .query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope)
        .append_pair("state", &expected_state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    app.opener()
        .open_url(authorization_url.as_str(), None::<&str>)
        .map_err(|_| {
            OAuthFailure::new(
                "NEXUS_SSO_BROWSER_FAILED",
                "The Nexus Mods authorization page could not be opened.",
            )
        })?;
    let code = wait_for_code(listener, &expected_state, &cancel)?;
    if cancel.load(Ordering::Acquire) {
        return Err(OAuthFailure::new(
            "NEXUS_SSO_CANCELLED",
            "Nexus Mods sign-in was cancelled.",
        ));
    }
    let parameters = [
        ("grant_type", "authorization_code"),
        ("redirect_uri", REDIRECT_URI),
        ("scope", scope.as_str()),
        ("client_id", client_id.as_str()),
        ("code", code.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    let tokens = token_request(&parameters, false, None)?;
    if cancel.load(Ordering::Acquire) {
        return Err(OAuthFailure::new(
            "NEXUS_SSO_CANCELLED",
            "Nexus Mods sign-in was cancelled.",
        ));
    }
    let user = validate_access_token(state, tokens.access_token.clone())?;
    let _mutation = state.nexus_oauth_refresh.lock().map_err(|_| {
        OAuthFailure::new(
            "NEXUS_SSO_FAILED",
            "Nexus Mods sign-in could not be completed.",
        )
    })?;
    if cancel.load(Ordering::Acquire) {
        return Err(OAuthFailure::new(
            "NEXUS_SSO_CANCELLED",
            "Nexus Mods sign-in was cancelled.",
        ));
    }
    store_tokens(state, &tokens)?;
    Ok(json!({
        "ok": true,
        "status": {
            "configured": true,
            "connected": true,
            "authMethod": "oauth-pkce",
            "ssoAvailable": true,
            "ssoPending": false,
            "personalKeyFallbackAllowed": false,
            "valid": true,
            "name": user.name.unwrap_or_else(|| "Nexus Mods user".to_owned()),
            "userId": user.user_id,
            "premium": user.is_premium.unwrap_or(false),
            "supporter": user.is_supporter.unwrap_or(false)
        }
    }))
}

pub fn start(app: &AppHandle, state: &AppState) -> Value {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let Ok(mut active) = state.nexus_oauth_cancel.lock() else {
            return OAuthFailure::new(
                "NEXUS_SSO_FAILED",
                "Nexus Mods sign-in could not be started.",
            )
            .response();
        };
        if active.is_some() {
            return OAuthFailure::new(
                "NEXUS_SSO_ALREADY_PENDING",
                "A Nexus Mods sign-in is already waiting for authorization.",
            )
            .response();
        }
        *active = Some(Arc::clone(&cancel));
    }
    let result = run_login(app, state, cancel);
    if let Ok(mut active) = state.nexus_oauth_cancel.lock() {
        *active = None;
    }
    result.unwrap_or_else(|error| error.response())
}

pub fn cancel(state: &AppState) -> bool {
    let Ok(active) = state.nexus_oauth_cancel.lock() else {
        return false;
    };
    let Some(cancel) = active.as_ref() else {
        return false;
    };
    !cancel.swap(true, Ordering::AcqRel)
}

pub fn pending(state: &AppState) -> bool {
    state
        .nexus_oauth_cancel
        .lock()
        .map(|active| active.is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_and_state_are_bounded() {
        let (verifier, challenge) = verifier_and_challenge();
        assert!((43..=128).contains(&verifier.len()));
        assert_eq!(challenge.len(), 43);
        assert_ne!(verifier, challenge);
        let state = random_state();
        assert!(constant_time_equal(&state, &state));
        assert!(!constant_time_equal(&state, "wrong"));
    }

    #[test]
    fn token_validation_rejects_legacy_api_keys() {
        let legacy = r#"{"schemaVersion":1,"accessToken":"short","refreshToken":"A-secure-looking-api-key-123456","tokenType":"Bearer","issuedAt":1,"expiresAt":2,"scope":""}"#;
        let parsed: OAuthTokens = serde_json::from_str(legacy).unwrap();
        assert!(validate_tokens(parsed).is_err());
    }

    #[test]
    fn callback_configuration_is_fixed_ipv4_loopback() {
        assert_eq!(REDIRECT_URI, "http://127.0.0.1:52817/callback");
        assert_eq!(CALLBACK_PORT, 52817);
        assert_eq!(CALLBACK_HOST, "127.0.0.1");
    }
}
