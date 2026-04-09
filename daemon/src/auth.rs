//! GitHub token validation.
//!
//! On every WebSocket handshake we take the bearer token the client
//! presented and ask GitHub two questions:
//!   1. GET /user                              — who is this token?
//!   2. GET /orgs/Clavigers/members/{login}    — are they in our org?
//! Both must succeed. If they do, the connection is accepted and we
//! return the GitHub login of the connecting user (for logging).

use serde_json::Value;

const REQUIRED_ORG: &str = "Clavigers";
const USER_AGENT: &str = "abrasive-daemon";

pub fn validate(token: &str) -> Result<String, String> {
    let agent = ureq::agent();

    // 1. Identify the user.
    let user_resp = agent
        .get("https://api.github.com/user")
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("github /user call failed: {e}"))?;

    let user_json: Value = user_resp
        .into_json()
        .map_err(|e| format!("parsing /user response: {e}"))?;

    let login = user_json
        .get("login")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "no login in /user response".to_string())?
        .to_string();

    // 2. Check org membership. 204 = member; everything else (404, 302) =
    //    not a member or token lacks read:org scope.
    let url = format!("https://api.github.com/orgs/{REQUIRED_ORG}/members/{login}");
    match agent
        .get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", USER_AGENT)
        .call()
    {
        Ok(resp) if resp.status() == 204 => Ok(login),
        Ok(resp) => Err(format!(
            "user '{login}' not a member of {REQUIRED_ORG} (status {})",
            resp.status()
        )),
        Err(ureq::Error::Status(_, _)) => Err(format!(
            "user '{login}' not a member of {REQUIRED_ORG}"
        )),
        Err(e) => Err(format!("membership check failed: {e}")),
    }
}
