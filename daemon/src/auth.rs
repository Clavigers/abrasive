//! GitHub token validation.
//!
//! On every WebSocket handshake we take the bearer token the client
//! presented and ask GitHub two questions:
//!   1. GET /user                              — who is this token?
//!   2. GET /orgs/Clavigers/members/{login}    — are they in our org?
//! Both must succeed. If they do, the connection is accepted and we
//! return the GitHub login of the connecting user (for logging).

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::constants::{REQUIRED_ORG, USER_AGENT};
use crate::errors::AuthError;

const USER_URL: &str = "https://api.github.com/user";
const CACHE_TTL: Duration = Duration::from_secs(300);

type TokenCache = Arc<Mutex<HashMap<[u8; 32], (String, Instant)>>>;

fn cache() -> &'static TokenCache {
    static CACHE: OnceLock<TokenCache> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

pub fn validate(token: &str) -> Result<String, AuthError> {
    let key = *blake3::hash(token.as_bytes()).as_bytes();
    if let Some(login) = lookup_cached(&key) {
        println!("[auth] token cache hit for '{login}'");
        return Ok(login);
    }
    let login = validate_against_github(token)?;
    println!("[auth] token cache miss, validated '{login}' against github");
    store_cached(key, login.clone());
    Ok(login)
}

fn lookup_cached(key: &[u8; 32]) -> Option<String> {
    let map = cache().lock().unwrap();
    let (login, expires) = map.get(key)?;
    (Instant::now() < *expires).then(|| login.clone())
}

fn store_cached(key: [u8; 32], login: String) {
    let mut map = cache().lock().unwrap();
    map.insert(key, (login, Instant::now() + CACHE_TTL));
}

fn validate_against_github(token: &str) -> Result<String, AuthError> {
    let agent = ureq::agent();
    let login = fetch_login(&agent, token)?;
    check_membership(&agent, token, &login)?;
    Ok(login)
}

fn fetch_login(agent: &ureq::Agent, token: &str) -> Result<String, AuthError> {
    let json: Value = github_get(agent, token, USER_URL)
        .map_err(AuthError::UserCall)?
        .into_json()
        .map_err(AuthError::UserResponseParse)?;
    json.get("login")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or(AuthError::NoLoginField)
}

fn check_membership(agent: &ureq::Agent, token: &str, login: &str) -> Result<(), AuthError> {
    let url = format!("https://api.github.com/orgs/{REQUIRED_ORG}/members/{login}");
    match github_get(agent, token, &url) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(404, _)) => Err(AuthError::NotMember {
            login: login.to_string(),
        }),
        Err(e) => Err(AuthError::MembershipCheck(e)),
    }
}

fn github_get(agent: &ureq::Agent, token: &str, url: &str) -> Result<ureq::Response, ureq::Error> {
    agent
        .get(url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", USER_AGENT)
        .call()
}
