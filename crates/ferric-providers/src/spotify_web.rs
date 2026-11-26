//! Spotify web-session library access.
//!
//! The public Web API path needs an app Client ID, and Spotify now requires the app owner to
//! hold an active Premium subscription plus a 5-account development allowlist. This module
//! takes the other route: the user signs in to Spotify's own web player once, and the
//! `sp_dc` session cookie that the browser already holds is exchanged for a short-lived
//! access token issued to that web session. No Client ID, no developer app, no quota.
//!
//! Scope of what this can do: read the signed-in user's own catalog (profile, Liked Songs,
//! playlists) so it can be mirrored into the local library. It does not obtain audio keys;
//! Spotify audio stays behind the account's own product tier.

use crate::{ProviderError, ProviderResult};
use serde::Deserialize;

/// Web player token endpoint. Responds with the session-bound access token when a valid
/// `sp_dc` cookie is presented.
const WEB_TOKEN_ENDPOINT: &str =
    "https://open.spotify.com/get_access_token?reason=transport&productType=web_player";

/// Spotify serves this endpoint only to browser-shaped clients; a non-browser UA is rejected.
const WEB_PLAYER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/131.0.0.0 Safari/537.36";

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct WebTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "isAnonymous", default)]
    is_anonymous: bool,
}

/// Extract the `sp_dc` value out of a raw cookie header (`a=1; sp_dc=XYZ; b=2`).
pub fn extract_sp_dc(cookie_header: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.trim() == "sp_dc" {
            let value = value.trim();
            if !value.is_empty() && value.len() <= 4096 {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Exchange a signed-in web session cookie for an access token bound to that account.
pub fn access_token_from_sp_dc(sp_dc: &str) -> ProviderResult<String> {
    let sp_dc = sp_dc.trim();
    if sp_dc.is_empty() {
        return Err(ProviderError::Metadata(
            "Spotify session cookie is empty".to_string(),
        ));
    }
    if sp_dc.len() > 4096 || sp_dc.bytes().any(|b| b.is_ascii_control() || b == b';') {
        return Err(ProviderError::Metadata(
            "Spotify session cookie is malformed".to_string(),
        ));
    }

    let resp = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .build(),
    )
    .get(WEB_TOKEN_ENDPOINT)
    .header("Cookie", &format!("sp_dc={sp_dc}"))
    .header("Accept", "application/json")
    .header("User-Agent", WEB_PLAYER_USER_AGENT)
    .header("Referer", "https://open.spotify.com/")
    .call()
    .map_err(|e| ProviderError::Http(format!("Spotify web token request failed: {e}")))?;

    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(format!("Could not read Spotify token body: {e}")))?;

    if status != 200 {
        return Err(ProviderError::Http(format!(
            "Spotify web token rejected the session (HTTP {status}). Sign in again."
        )));
    }

    let parsed: WebTokenResponse = serde_json::from_slice(&bytes)
        .map_err(|e| ProviderError::Http(format!("Invalid Spotify token JSON: {e}")))?;

    if parsed.is_anonymous {
        return Err(ProviderError::Metadata(
            "Spotify returned an anonymous token; the session is not signed in".to_string(),
        ));
    }

    match parsed.access_token {
        Some(token) if !token.trim().is_empty() => Ok(token),
        _ => Err(ProviderError::Http(
            "Spotify web token response had no access token".to_string(),
        )),
    }
}

/// Parse the token JSON produced by the in-page `get_access_token` fetch.
///
/// WebView2's `ExecuteScript` returns the result JSON-encoded, so the payload may arrive as a
/// quoted, escaped string rather than raw JSON.
pub fn parse_web_token_json(raw: &str) -> ProviderResult<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ProviderError::Http(
            "Spotify returned an empty token response".to_string(),
        ));
    }
    let text = serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_string());
    let text = text.trim();
    if let Some(err) = text.strip_prefix("ERR:") {
        return Err(ProviderError::Http(format!(
            "Spotify token fetch failed inside the page: {err}"
        )));
    }
    let parsed: WebTokenResponse = serde_json::from_str(text)
        .map_err(|e| ProviderError::Http(format!("Invalid Spotify token JSON: {e}")))?;
    if parsed.is_anonymous {
        return Err(ProviderError::Metadata(
            "Spotify returned an anonymous token; the session is not signed in".to_string(),
        ));
    }
    match parsed.access_token {
        Some(token) if !token.trim().is_empty() => Ok(token),
        _ => Err(ProviderError::Http(
            "Spotify token response had no access token".to_string(),
        )),
    }
}

/// Read the signed-in user's profile, Liked Songs and playlists into a sync report.
pub fn sync_catalog_via_sp_dc(sp_dc: &str) -> ProviderResult<crate::oauth::CloudSyncReport> {
    let token = access_token_from_sp_dc(sp_dc)?;
    crate::oauth::sync_spotify_catalog(&token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sp_dc_from_cookie_header() {
        let header = "sp_t=abc; sp_dc=AQBc123XYZ; sp_key=def";
        assert_eq!(extract_sp_dc(header).as_deref(), Some("AQBc123XYZ"));
    }

    #[test]
    fn ignores_missing_or_empty_sp_dc() {
        assert_eq!(extract_sp_dc("sp_t=abc; sp_key=def"), None);
        assert_eq!(extract_sp_dc("sp_dc=; sp_t=abc"), None);
        assert_eq!(extract_sp_dc(""), None);
    }

    #[test]
    fn rejects_malformed_cookie_value() {
        assert!(access_token_from_sp_dc("").is_err());
        assert!(access_token_from_sp_dc("bad;value").is_err());
    }

    #[test]
    fn parses_raw_and_quoted_token_json() {
        let raw = r#"{"accessToken":"tok_123","isAnonymous":false}"#;
        assert_eq!(parse_web_token_json(raw).unwrap(), "tok_123");

        let quoted = serde_json::to_string(raw).unwrap();
        assert_eq!(parse_web_token_json(&quoted).unwrap(), "tok_123");
    }

    #[test]
    fn rejects_anonymous_and_error_payloads() {
        let anon = r#"{"accessToken":"x","isAnonymous":true}"#;
        assert!(parse_web_token_json(anon).is_err());
        assert!(parse_web_token_json("ERR:boom").is_err());
        assert!(parse_web_token_json("").is_err());
    }
}
