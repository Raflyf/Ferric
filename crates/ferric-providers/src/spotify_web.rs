//! Spotify web-session library access.
//!
//! The public Web API path needs an app Client ID, and Spotify now requires the app owner to
//! hold an active Premium subscription plus a 5-account development allowlist. This module
//! takes the other route: the user signs in to Spotify's own web player once and the desktop
//! login window captures the access token that the player fetched for itself.
//!
//! The token endpoint is deliberately not called from Rust anymore: it answers with HTML to
//! clients that do not reproduce the web player's own request, so the request has to stay in
//! the page. This module only validates the captured JSON.
//!
//! Scope of what this can do: read the signed-in user's own catalog (profile, Liked Songs,
//! playlists) so it can be mirrored into the local library. It does not obtain audio keys;
//! Spotify audio stays behind the account's own product tier.

use crate::{ProviderError, ProviderResult};
use serde::Deserialize;

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

/// Parse the token JSON captured from the signed-in web player.
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
            "Spotify token capture failed inside the page: {err}"
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
