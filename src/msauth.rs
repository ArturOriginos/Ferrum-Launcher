//! Sign-in with a Microsoft account: OAuth device code flow and the Xbox / Minecraft token chain.
//!
//! The launcher never sees the password: the user signs in on Microsoft's own page and types a short code.
//! The refresh token and the Minecraft token are kept in a separate file per account (`auth/<uuid>.json`,
//! readable only by the current user on Linux and macOS), never in `config.json` and never in logs.
//!
//! Minecraft services only accept applications that Mojang has approved, so the Azure application ID
//! has to come from the user (or be built in with `FERRUM_MS_CLIENT_ID`). See README → Microsoft sign-in.

use crate::config::{auth_dir, now};
use crate::net;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MS_BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const SCOPE: &str = "XboxLive.signin offline_access";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const FALLBACK_URL: &str = "https://www.microsoft.com/link";

// ---------- Application ID ----------

/// The Azure application (client) ID: from the settings, otherwise the one built into this build.
/// The ID is public (there is no client secret), so it is safe to build it in.
pub fn client_id(from_settings: &str) -> String {
    let s = from_settings.trim();
    if !s.is_empty() {
        return s.to_string();
    }
    option_env!("FERRUM_MS_CLIENT_ID").unwrap_or("").trim().to_string()
}

/// A GUID like `00000000-0000-0000-0000-000000000000`.
pub fn valid_client_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Only Microsoft's own pages may be opened for the user.
pub fn safe_verification_url(u: &str) -> bool {
    let Some(rest) = u.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.contains(':') {
        return false;
    }
    ["microsoft.com", "live.com", "microsoftonline.com"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

// ---------- Stored session ----------

#[derive(Serialize, Deserialize, Clone)]
struct Stored {
    refresh_token: String,
    mc_token: String,
    mc_expires_at: u64,
    name: String,
    uuid: String,
}

fn stored_path(uuid: &str) -> PathBuf {
    let safe: String = uuid
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    auth_dir().join(format!("{safe}.json"))
}

fn load(uuid: &str) -> Option<Stored> {
    serde_json::from_slice(&std::fs::read(stored_path(uuid)).ok()?).ok()
}

fn save(s: &Stored) -> Result<()> {
    let data = serde_json::to_vec_pretty(s)?;
    write_private(&stored_path(&s.uuid), &data)
}

/// Removes the saved sign-in of an account (when the account is deleted).
pub fn forget(uuid: &str) {
    let _ = std::fs::remove_file(stored_path(uuid));
}

/// Writes through a temporary file; on Linux and macOS the file is readable by the owner only.
fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(data)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// What the launcher needs to start the game with this account.
#[derive(Clone)]
pub struct LaunchSession {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
}

// ---------- Microsoft OAuth ----------

struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
    expires_in: u64,
}

struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
}

enum Poll {
    Pending,
    SlowDown,
    Done(Tokens),
}

fn read(resp: Response) -> (u16, Value) {
    let status = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// A readable message for an OAuth error answer (never contains tokens).
fn oauth_error(v: &Value) -> String {
    let code = v["error"].as_str().unwrap_or("unknown_error");
    let desc = v["error_description"]
        .as_str()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("");
    let hint = if desc.contains("AADSTS70002")
        || desc.contains("AADSTS7000218")
        || code == "invalid_client"
    {
        " Turn on \"Allow public client flows\" for the application in the Azure portal."
    } else if desc.contains("AADSTS700016") {
        " The application ID was not found. Check that it is correct."
    } else {
        ""
    };
    format!("Microsoft refused the request ({code}). {desc}{hint}")
}

fn parse_device_code(v: &Value) -> Result<DeviceCode> {
    let s = |k: &str| v[k].as_str().map(str::to_string);
    Ok(DeviceCode {
        device_code: s("device_code")
            .ok_or_else(|| anyhow!("Microsoft did not return a device code"))?,
        user_code: s("user_code").ok_or_else(|| anyhow!("Microsoft did not return a user code"))?,
        verification_uri: s("verification_uri").unwrap_or_else(|| FALLBACK_URL.to_string()),
        interval: v["interval"].as_u64().unwrap_or(5).max(1),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
    })
}

fn parse_tokens(v: &Value) -> Option<Tokens> {
    Some(Tokens {
        access_token: v["access_token"].as_str()?.to_string(),
        refresh_token: v["refresh_token"].as_str().map(str::to_string),
    })
}

fn request_device_code(http: &Client, client_id: &str) -> Result<DeviceCode> {
    let resp = http
        .post(format!("{MS_BASE}/devicecode"))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .context("Could not reach Microsoft")?;
    let (status, v) = read(resp);
    if status != 200 {
        bail!("{}", oauth_error(&v));
    }
    parse_device_code(&v)
}

fn poll_token(http: &Client, client_id: &str, device_code: &str) -> Result<Poll> {
    let resp = http
        .post(format!("{MS_BASE}/token"))
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id),
            ("device_code", device_code),
        ])
        .send()
        .context("Could not reach Microsoft")?;
    let (status, v) = read(resp);
    if status == 200 {
        return parse_tokens(&v)
            .map(Poll::Done)
            .ok_or_else(|| anyhow!("Microsoft returned an unexpected answer"));
    }
    match v["error"].as_str().unwrap_or("") {
        "authorization_pending" => Ok(Poll::Pending),
        "slow_down" => Ok(Poll::SlowDown),
        "expired_token" => bail!("The code has expired. Start the sign-in again."),
        "authorization_declined" | "access_denied" => bail!("The sign-in was declined."),
        _ => bail!("{}", oauth_error(&v)),
    }
}

fn refresh(http: &Client, client_id: &str, refresh_token: &str) -> Result<Tokens> {
    let resp = http
        .post(format!("{MS_BASE}/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ])
        .send()
        .context("Could not reach Microsoft")?;
    let (status, v) = read(resp);
    if status == 200 {
        return parse_tokens(&v).ok_or_else(|| anyhow!("Microsoft returned an unexpected answer"));
    }
    if v["error"].as_str() == Some("invalid_grant") {
        bail!("The Microsoft sign-in has expired. Remove the account and sign in again.");
    }
    bail!("{}", oauth_error(&v))
}

// ---------- Xbox Live, XSTS, Minecraft ----------

fn xbl_body(ms_access: &str) -> Value {
    json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={ms_access}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    })
}

fn xsts_body(xbl: &str) -> Value {
    json!({
        "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl] },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    })
}

fn mc_login_body(uhs: &str, xsts: &str) -> Value {
    json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts}") })
}

fn xsts_error_message(code: u64) -> String {
    match code {
        2148916233 => "This Microsoft account has no Xbox profile. Sign in once at minecraft.net or xbox.com to create it, then try again.".to_string(),
        2148916235 => "Xbox Live is not available in the country of this account.".to_string(),
        2148916236 | 2148916237 => "This account needs adult verification on the Xbox site.".to_string(),
        2148916238 => "This is a child account. An adult has to add it to a Microsoft Family before it can play.".to_string(),
        n => format!("Xbox services refused the sign-in (code {n})."),
    }
}

fn xbox_token(http: &Client, ms_access: &str) -> Result<String> {
    let resp = http
        .post(XBL_URL)
        .header("Accept", "application/json")
        .json(&xbl_body(ms_access))
        .send()
        .context("Could not reach Xbox Live")?;
    let (status, v) = read(resp);
    if status != 200 {
        bail!("Xbox Live refused the sign-in (HTTP {status})");
    }
    v["Token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Xbox Live returned an unexpected answer"))
}

/// Returns (XSTS token, user hash).
fn xsts_token(http: &Client, xbl: &str) -> Result<(String, String)> {
    let resp = http
        .post(XSTS_URL)
        .header("Accept", "application/json")
        .json(&xsts_body(xbl))
        .send()
        .context("Could not reach Xbox services")?;
    let (status, v) = read(resp);
    if status != 200 {
        match v["XErr"].as_u64() {
            Some(c) => bail!("{}", xsts_error_message(c)),
            None => bail!("Xbox services refused the sign-in (HTTP {status})"),
        }
    }
    let token = v["Token"].as_str().map(str::to_string);
    let uhs = v["DisplayClaims"]["xui"][0]["uhs"].as_str().map(str::to_string);
    match (token, uhs) {
        (Some(t), Some(u)) => Ok((t, u)),
        _ => bail!("Xbox services returned an unexpected answer"),
    }
}

/// Returns (Minecraft access token, lifetime in seconds).
fn minecraft_token(http: &Client, uhs: &str, xsts: &str) -> Result<(String, u64)> {
    let resp = http
        .post(MC_LOGIN_URL)
        .json(&mc_login_body(uhs, xsts))
        .send()
        .context("Could not reach Minecraft services")?;
    let (status, v) = read(resp);
    if status == 403 {
        bail!("Minecraft services rejected this application (Invalid app registration). The Azure application has to be approved by Mojang first, see README → Microsoft sign-in.");
    }
    if status != 200 {
        bail!("Minecraft services refused the sign-in (HTTP {status})");
    }
    let token = v["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Minecraft services returned an unexpected answer"))?;
    Ok((token, v["expires_in"].as_u64().unwrap_or(86_400)))
}

/// Returns (uuid with dashes, name).
fn profile(http: &Client, mc_token: &str) -> Result<(String, String)> {
    let resp = http
        .get(MC_PROFILE_URL)
        .bearer_auth(mc_token)
        .send()
        .context("Could not reach Minecraft services")?;
    let (status, v) = read(resp);
    if status == 404 {
        bail!("This Microsoft account does not own Minecraft: Java Edition (no Minecraft profile was found).");
    }
    if status != 200 {
        bail!("Minecraft services refused the profile request (HTTP {status})");
    }
    match (v["id"].as_str(), v["name"].as_str()) {
        (Some(id), Some(name)) => Ok((dashed_uuid(id), name.to_string())),
        _ => bail!("Minecraft services returned an unexpected profile"),
    }
}

pub fn dashed_uuid(id: &str) -> String {
    let h: String = id.chars().filter(|c| *c != '-').collect::<String>().to_lowercase();
    if h.len() != 32 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return id.to_string();
    }
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

struct Chain {
    mc_token: String,
    expires_at: u64,
    name: String,
    uuid: String,
}

fn run_chain(http: &Client, ms_access: &str) -> Result<Chain> {
    let xbl = xbox_token(http, ms_access)?;
    let (xsts, uhs) = xsts_token(http, &xbl)?;
    let (mc, ttl) = minecraft_token(http, &uhs, &xsts)?;
    let (uuid, name) = profile(http, &mc)?;
    Ok(Chain {
        mc_token: mc,
        expires_at: now() + ttl,
        name,
        uuid,
    })
}

// ---------- Interactive sign-in (runs on a background thread) ----------

#[derive(Clone)]
pub enum LoginState {
    Starting,
    Waiting {
        user_code: String,
        url: String,
        expires_at: u64,
    },
    Finishing,
    Done {
        name: String,
        uuid: String,
    },
    Failed(String),
    Cancelled,
}

#[derive(Clone)]
pub struct LoginHandle {
    state: Arc<Mutex<LoginState>>,
    cancel: Arc<AtomicBool>,
}

impl LoginHandle {
    pub fn state(&self) -> LoginState {
        match self.state.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    fn set(&self, s: LoginState) {
        match self.state.lock() {
            Ok(mut g) => *g = s,
            Err(p) => *p.into_inner() = s,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

pub fn start_login(http: Client, client_id: String) -> LoginHandle {
    let h = LoginHandle {
        state: Arc::new(Mutex::new(LoginState::Starting)),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let (h_panic, h_run) = (h.clone(), h.clone());
    net::spawn_guarded(
        move |m| h_panic.set(LoginState::Failed(m)),
        move || {
            let end = match login_flow(&http, &client_id, &h_run) {
                Ok((name, uuid)) => LoginState::Done { name, uuid },
                Err(_) if h_run.cancelled() => LoginState::Cancelled,
                Err(e) => LoginState::Failed(format!("{e:#}")),
            };
            h_run.set(end);
        },
    );
    h
}

fn login_flow(http: &Client, client_id: &str, h: &LoginHandle) -> Result<(String, String)> {
    let dc = request_device_code(http, client_id)?;
    let url = if safe_verification_url(&dc.verification_uri) {
        dc.verification_uri.clone()
    } else {
        FALLBACK_URL.to_string()
    };
    let deadline = now() + dc.expires_in;
    h.set(LoginState::Waiting {
        user_code: dc.user_code.clone(),
        url,
        expires_at: deadline,
    });

    let mut interval = dc.interval;
    let tokens = loop {
        // wait in small steps so that "Cancel" reacts quickly
        let mut waited_ms: u64 = 0;
        while waited_ms < interval * 1000 {
            if h.cancelled() {
                bail!("Cancelled");
            }
            std::thread::sleep(Duration::from_millis(250));
            waited_ms += 250;
        }
        if now() >= deadline {
            bail!("The code has expired. Start the sign-in again.");
        }
        match poll_token(http, client_id, &dc.device_code)? {
            Poll::Pending => {}
            Poll::SlowDown => interval += 5,
            Poll::Done(t) => break t,
        }
    };

    h.set(LoginState::Finishing);
    let refresh_token = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| anyhow!("Microsoft did not return a refresh token"))?;
    let chain = run_chain(http, &tokens.access_token)?;
    save(&Stored {
        refresh_token,
        mc_token: chain.mc_token,
        mc_expires_at: chain.expires_at,
        name: chain.name.clone(),
        uuid: chain.uuid.clone(),
    })?;
    Ok((chain.name, chain.uuid))
}

// ---------- Before launching the game ----------

/// A valid Minecraft token for the account. A cached token is used while it has more than
/// five minutes left; otherwise it is renewed silently with the saved refresh token.
pub fn ensure_session(http: &Client, client_id: &str, uuid: &str) -> Result<LaunchSession> {
    let mut s = load(uuid).ok_or_else(|| {
        anyhow!("There is no saved Microsoft sign-in for this account. Remove it and sign in again.")
    })?;
    if !s.mc_token.is_empty() && s.mc_expires_at > now() + 300 {
        return Ok(LaunchSession {
            name: s.name,
            uuid: s.uuid,
            access_token: s.mc_token,
        });
    }
    if client_id.is_empty() {
        bail!("The Microsoft application ID is not set. Enter it in Accounts → Microsoft account.");
    }
    let tokens = refresh(http, client_id, &s.refresh_token)?;
    if let Some(r) = &tokens.refresh_token {
        // keep the newest refresh token even if the next steps fail
        s.refresh_token = r.clone();
        let _ = save(&s);
    }
    let chain = run_chain(http, &tokens.access_token)?;
    s.mc_token = chain.mc_token;
    s.mc_expires_at = chain.expires_at;
    s.name = chain.name;
    s.uuid = chain.uuid;
    save(&s)?;
    Ok(LaunchSession {
        name: s.name,
        uuid: s.uuid,
        access_token: s.mc_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_format() {
        assert!(valid_client_id("12345678-abcd-4ef0-9abc-0123456789ab"));
        assert!(valid_client_id("ABCDEF12-ABCD-ABCD-ABCD-ABCDEF123456"));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("12345678abcd4ef09abc0123456789ab"));
        assert!(!valid_client_id("12345678-abcd-4ef0-9abc-0123456789zz"));
        assert!(!valid_client_id("12345678-abcd-4ef0-9abc-0123456789abc"));
    }

    #[test]
    fn settings_client_id_wins_and_is_trimmed() {
        assert_eq!(client_id("  abc  "), "abc");
    }

    #[test]
    fn only_microsoft_pages_are_opened() {
        assert!(safe_verification_url("https://www.microsoft.com/link"));
        assert!(safe_verification_url("https://microsoft.com/link"));
        assert!(safe_verification_url("https://login.live.com/oauth20_remoteconnect.srf"));
        assert!(!safe_verification_url("http://www.microsoft.com/link"));
        assert!(!safe_verification_url("https://microsoft.com.evil.com/link"));
        assert!(!safe_verification_url("https://evil.com/microsoft.com"));
        assert!(!safe_verification_url("https://www.microsoft.com@evil.com/"));
        assert!(!safe_verification_url("https://www.microsoft.com:8443/"));
        assert!(!safe_verification_url("file:///etc/passwd"));
        assert!(!safe_verification_url(""));
    }

    #[test]
    fn uuid_gets_dashes() {
        assert_eq!(
            dashed_uuid("0123456789ABCDEF0123456789abcdef"),
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(
            dashed_uuid("01234567-89ab-cdef-0123-456789abcdef"),
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(dashed_uuid("short"), "short");
    }

    #[test]
    fn xsts_errors_are_explained() {
        assert!(xsts_error_message(2148916233).contains("no Xbox profile"));
        assert!(xsts_error_message(2148916235).contains("country"));
        assert!(xsts_error_message(2148916238).contains("Family"));
        assert!(xsts_error_message(1).contains("code 1"));
    }

    #[test]
    fn device_code_is_parsed() {
        let v = json!({
            "device_code": "DC", "user_code": "ABCD1234",
            "verification_uri": "https://www.microsoft.com/link",
            "expires_in": 900, "interval": 5
        });
        let d = parse_device_code(&v).unwrap();
        assert_eq!(d.device_code, "DC");
        assert_eq!(d.user_code, "ABCD1234");
        assert_eq!(d.interval, 5);
        assert_eq!(d.expires_in, 900);
        assert!(parse_device_code(&json!({})).is_err());
    }

    #[test]
    fn tokens_are_parsed() {
        let t = parse_tokens(&json!({"access_token": "a", "refresh_token": "r"})).unwrap();
        assert_eq!(t.access_token, "a");
        assert_eq!(t.refresh_token.as_deref(), Some("r"));
        assert!(parse_tokens(&json!({"error": "x"})).is_none());
    }

    #[test]
    fn request_bodies_have_the_expected_shape() {
        let b = xbl_body("MSTOKEN");
        assert_eq!(b["Properties"]["RpsTicket"], "d=MSTOKEN");
        assert_eq!(b["RelyingParty"], "http://auth.xboxlive.com");
        let b = xsts_body("XBL");
        assert_eq!(b["Properties"]["UserTokens"][0], "XBL");
        assert_eq!(b["RelyingParty"], "rp://api.minecraftservices.com/");
        let b = mc_login_body("HASH", "XSTS");
        assert_eq!(b["identityToken"], "XBL3.0 x=HASH;XSTS");
    }

    #[test]
    fn oauth_errors_give_a_hint_without_secrets() {
        let v = json!({"error": "invalid_client", "error_description": "AADSTS7000218: The request body must contain...\r\nTrace ID: x"});
        let m = oauth_error(&v);
        assert!(m.contains("public client flows"));
        assert!(!m.contains("Trace ID"));
    }
}
