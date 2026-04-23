//! Abrasive API token validation against Supabase.
//!
//! On every WebSocket handshake we SHA-256 the bearer token the client
//! presented and look it up in `public.api_tokens` via PostgREST using
//! the service-role key. If present and not expired, the connection is
//! accepted and we return the Supabase user_id.
//!
//! Tokens are cached for 5 minutes to avoid hammering the DB on every
//! build request.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::constants::USER_AGENT;
use crate::errors::AuthError;

const CACHE_TTL: Duration = Duration::from_secs(300);
const TOKEN_PREFIX: &str = "abrasive_";

#[derive(Deserialize)]
struct TokenRow {
    user_id: String,
    expires_at: Option<String>,
}

type TokenCache = Arc<Mutex<HashMap<[u8; 32], (String, Instant)>>>;

fn cache() -> &'static TokenCache {
    static CACHE: OnceLock<TokenCache> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

pub fn validate(token: &str) -> Result<String, AuthError> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Err(AuthError::InvalidTokenFormat);
    }
    let hash = Sha256::digest(token.as_bytes());
    let cache_key: [u8; 32] = hash.into();

    if let Some(user_id) = lookup_cached(&cache_key) {
        return Ok(user_id);
    }

    let hash_hex = hex_encode(&hash);
    let row = fetch_token_row(&hash_hex)?;
    check_not_expired(&row)?;

    store_cached(cache_key, row.user_id.clone());
    Ok(row.user_id)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn lookup_cached(key: &[u8; 32]) -> Option<String> {
    let map = cache().lock().unwrap();
    let (user_id, expires) = map.get(key)?;
    (Instant::now() < *expires).then(|| user_id.clone())
}

fn store_cached(key: [u8; 32], user_id: String) {
    let mut map = cache().lock().unwrap();
    map.insert(key, (user_id, Instant::now() + CACHE_TTL));
}

fn supabase_url() -> Result<String, AuthError> {
    env::var("SUPABASE_URL").map_err(|_| AuthError::MissingEnv("SUPABASE_URL"))
}

fn supabase_service_key() -> Result<String, AuthError> {
    env::var("SUPABASE_SERVICE_ROLE_KEY")
        .map_err(|_| AuthError::MissingEnv("SUPABASE_SERVICE_ROLE_KEY"))
}

fn fetch_token_row(hash_hex: &str) -> Result<TokenRow, AuthError> {
    let base = supabase_url()?;
    let key = supabase_service_key()?;
    let url = format!(
        "{base}/rest/v1/api_tokens?token_hash=eq.{hash_hex}&select=user_id,expires_at&limit=1"
    );

    let rows: Vec<TokenRow> = ureq::get(&url)
        .set("apikey", &key)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Accept", "application/json")
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(AuthError::SupabaseCall)?
        .into_json()
        .map_err(AuthError::SupabaseParseResponse)?;

    rows.into_iter().next().ok_or(AuthError::UnknownToken)
}

fn check_not_expired(row: &TokenRow) -> Result<(), AuthError> {
    let Some(expires_at) = &row.expires_at else {
        return Ok(());
    };
    let expires = OffsetDateTime::parse(expires_at, &Rfc3339)
        .map_err(|e| AuthError::TimestampParse(e.to_string()))?;
    if expires <= OffsetDateTime::now_utc() {
        return Err(AuthError::TokenExpired);
    }
    Ok(())
}
