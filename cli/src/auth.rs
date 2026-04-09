//! GitHub OAuth device flow + token cache.
//!
//! On first use the CLI runs the device flow: it asks GitHub for a
//! short user code, prints it along with a URL, and polls until the
//! user authorizes in their browser. The resulting token is cached at
//! ~/.config/abrasive/token and reused on subsequent invocations.
//!
//! The token is a real per-user GitHub token; the daemon validates it
//! against the GitHub API on every connection (see daemon/src/auth.rs).

use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::errors::{CliError, CliResult};

/// Public OAuth App client_id for the "Claviger" GitHub OAuth App.
/// Not a secret — it identifies the app to GitHub. Device flow does
/// not use a client secret.
const GITHUB_CLIENT_ID: &str = "Ov23liPnfnBs67h2r1vz";

/// Scopes the daemon needs to verify org membership.
const SCOPES: &str = "read:org";

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[derive(Deserialize)]
struct DeviceCodeResp {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
}

/// Returns a GitHub access token, running the device flow if necessary.
pub fn token() -> CliResult<String> {
    if let Some(t) = read_cached_token() {
        return Ok(t);
    }
    login()
}

/// Always runs the device flow, replacing any cached token.
pub fn login() -> CliResult<String> {
    let token = device_flow()?;
    if let Err(e) = write_cached_token(&token) {
        eprintln!("[auth] warning: could not cache token: {e}");
    }
    Ok(token)
}

fn device_flow() -> CliResult<String> {
    let agent = ureq::agent();

    // Step 1: request a device + user code.
    let resp: DeviceCodeResp = agent
        .post(DEVICE_CODE_URL)
        .set("Accept", "application/json")
        .send_form(&[("client_id", GITHUB_CLIENT_ID), ("scope", SCOPES)])
        .map_err(|e| CliError::auth(format!("device code request failed: {e}")))?
        .into_json()
        .map_err(|e| CliError::auth(format!("device code response parse failed: {e}")))?;

    eprintln!();
    eprintln!("To authenticate, visit:");
    eprintln!("    {}", resp.verification_uri);
    eprintln!("And enter the code:");
    eprintln!("    {}", resp.user_code);
    eprintln!();
    eprintln!("Waiting for authorization...");
    let _ = std::io::stderr().flush();

    // Step 2: poll for the access token.
    let mut interval = Duration::from_secs(resp.interval.max(1));
    loop {
        thread::sleep(interval);

        let json: Value = agent
            .post(ACCESS_TOKEN_URL)
            .set("Accept", "application/json")
            .send_form(&[
                ("client_id", GITHUB_CLIENT_ID),
                ("device_code", &resp.device_code),
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code",
                ),
            ])
            .map_err(|e| CliError::auth(format!("token poll failed: {e}")))?
            .into_json()
            .map_err(|e| CliError::auth(format!("token poll parse failed: {e}")))?;

        if let Some(token) = json.get("access_token").and_then(|v| v.as_str()) {
            eprintln!("[auth] success");
            return Ok(token.to_string());
        }

        match json.get("error").and_then(|v| v.as_str()) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += Duration::from_secs(5);
                continue;
            }
            Some("expired_token") => {
                return Err(CliError::auth(
                    "device code expired before authorization; run again".into(),
                ));
            }
            Some("access_denied") => {
                return Err(CliError::auth("authorization denied".into()));
            }
            Some(other) => {
                return Err(CliError::auth(format!("github error: {other}")));
            }
            None => {
                return Err(CliError::auth(format!("unexpected token response: {json}")));
            }
        }
    }
}

fn token_path() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("abrasive").join("token"))
}

fn read_cached_token() -> Option<String> {
    let path = token_path()?;
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn write_cached_token(token: &str) -> std::io::Result<()> {
    let path = token_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no HOME"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
