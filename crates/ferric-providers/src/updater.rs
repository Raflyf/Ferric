use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const GITHUB_REPO: &str = "Raflyf/Ferric";
const MAX_RELEASE_JSON_BYTES: usize = 128 * 1024;
const MAX_CHECKSUMS_BYTES: usize = 32 * 1024;
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const PHASE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag: String,
    pub version: String,
    pub release_url: String,
    pub summary: String,
}

#[derive(Clone, Debug)]
pub enum DownloadEvent {
    Progress(Option<u8>),
    Verifying,
    Ready { path: PathBuf, sha256: [u8; 32] },
    Failed(String),
}

pub fn normalize_tag(tag: &str) -> Result<String, String> {
    let parsed = tag
        .strip_prefix('v')
        .and_then(parse_version)
        .ok_or_else(|| "Release tag must be canonical vX.Y.Z".to_string())?;
    Ok(format!("v{}.{}.{}", parsed.0, parsed.1, parsed.2))
}

pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split('.');
    let mut next_num = || {
        let part = parts.next()?;
        if part.is_empty()
            || !part.bytes().all(|c| c.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        part.parse::<u32>().ok()
    };
    let value = (next_num()?, next_num()?, next_num()?);
    if parts.next().is_some() {
        None
    } else {
        Some(value)
    }
}

pub fn release_page_url(tag: Option<&str>) -> String {
    match tag.and_then(|t| normalize_tag(t).ok()) {
        Some(canonical) => format!("https://github.com/{GITHUB_REPO}/releases/tag/{canonical}"),
        None => format!("https://github.com/{GITHUB_REPO}/releases"),
    }
}

pub fn installer_filename(tag: &str) -> Result<String, String> {
    let canonical = normalize_tag(tag)?;
    Ok(format!("ferricsetup-{canonical}.msi"))
}

pub fn allowed_download_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
    )
}

pub fn validate_https_download_url(url: &str) -> Result<(&str, &str), String> {
    if url.bytes().any(|b| b.is_ascii_control()) {
        return Err("Invalid control characters in download URL".to_string());
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "Untrusted update download scheme".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains(':')
        || authority.contains(['\\', ' '])
        || !allowed_download_host(authority)
    {
        return Err("Untrusted update download host".to_string());
    }
    if authority.eq_ignore_ascii_case("github.com") {
        let expected_prefix = format!("/{GITHUB_REPO}/");
        if !path
            .get(..expected_prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(&expected_prefix))
            || path.contains("..")
        {
            return Err("Untrusted GitHub repository path".to_string());
        }
    }
    Ok((authority, path))
}

pub fn parse_sha256_hex(hex: &str) -> Result<[u8; 32], String> {
    let hex = hex.trim();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid SHA256SUMS digest entry".to_string());
    }
    let mut digest = [0u8; 32];
    for (i, slot) in digest.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "Invalid SHA256SUMS digest entry".to_string())?;
    }
    Ok(digest)
}

pub fn parse_sha256sums(text: &str, expected_file: &str) -> Result<[u8; 32], String> {
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((hex_part, file_part)) = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .or_else(|| line.split_once('\t'))
        else {
            continue;
        };
        if file_part.trim() != expected_file {
            continue;
        }
        return parse_sha256_hex(hex_part);
    }
    Err(format!("Missing checksum entry for {expected_file}"))
}

pub fn resolve_installer_from_sums(
    sums_text: &str,
    tag: &str,
) -> Result<(String, [u8; 32]), String> {
    let canonical = normalize_tag(tag)?;
    let msi_name = format!("ferricsetup-{canonical}.msi");
    let digest = parse_sha256sums(sums_text, &msi_name)?;
    Ok((msi_name, digest))
}

pub fn sha256_file(path: &Path) -> Result<[u8; 32], String> {
    let mut file = File::open(path).map_err(|e| format!("Could not read downloaded file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Could not hash downloaded file: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Ok(out)
}

/// Strictly parse GitHub `/releases/latest` JSON against `current_version`.
/// Rejects drafts, prereleases, non-canonical tags, and malformed types.
pub fn parse_latest_release(
    json_text: &str,
    current_version: &str,
) -> Result<Option<ReleaseInfo>, String> {
    let val: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| format!("Invalid release JSON: {e}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| "Release payload is not a JSON object".to_string())?;
    let draft = obj
        .get("draft")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "Release draft field is missing or invalid".to_string())?;
    let prerelease = obj
        .get("prerelease")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "Release prerelease field is missing or invalid".to_string())?;
    let tag_raw = obj
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Release tag is missing or invalid".to_string())?;
    if draft || prerelease {
        return Ok(None);
    }
    let remote = tag_raw
        .strip_prefix('v')
        .and_then(parse_version)
        .ok_or_else(|| "Release tag must be canonical vX.Y.Z".to_string())?;
    let local = parse_version(current_version)
        .ok_or_else(|| "Application version is not X.Y.Z".to_string())?;
    if remote <= local {
        return Ok(None);
    }

    let version = format!("{}.{}.{}", remote.0, remote.1, remote.2);
    let tag = format!("v{version}");
    let summary = obj
        .get("body")
        .and_then(serde_json::Value::as_str)
        .map(extract_short_summary)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            format!("You are on {current_version}. Read what changed, or update now.")
        });
    Ok(Some(ReleaseInfo {
        release_url: release_page_url(Some(&tag)),
        tag,
        version,
        summary,
    }))
}

fn extract_short_summary(body: &str) -> String {
    let mut lines = Vec::new();
    for raw in body.lines() {
        let trimmed = raw
            .trim()
            .trim_start_matches('#')
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim();
        if trimmed.is_empty() || trimmed.starts_with("---") || trimmed.starts_with("http") {
            continue;
        }
        lines.push(trimmed);
        if lines.len() >= 2 {
            break;
        }
    }
    let joined = lines.join(" · ");
    if joined.chars().count() > 140 {
        let cut: String = joined.chars().take(137).collect();
        format!("{cut}...")
    } else {
        joined
    }
}

pub fn format_check_status(
    result: &Result<Option<ReleaseInfo>, String>,
    current_version: &str,
) -> String {
    match result {
        Ok(Some(info)) => format!(
            "Update available: {} (current v{current_version})",
            info.tag
        ),
        Ok(None) => format!("You are using the latest version of Ferric (v{current_version})."),
        Err(err) => format!("Update check failed: {err}"),
    }
}

fn check_cancelled(cancelled: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Update operation cancelled".to_string());
    }
    if Instant::now() >= deadline {
        return Err("Update operation timed out".to_string());
    }
    Ok(())
}

fn build_strict_agent(total_timeout: Duration, phase_timeout: Duration) -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .timeout_connect(Some(phase_timeout))
        .timeout_recv_response(Some(phase_timeout))
        .timeout_global(Some(total_timeout))
        .build();
    ureq::Agent::new_with_config(cfg)
}

/// Query GitHub `/releases/latest` for a newer stable Ferric release.
/// Sends only `User-Agent: Ferric/<current_version>` (no user data or library paths).
pub fn check_latest_release(
    current_version: &str,
    cancelled: &AtomicBool,
) -> Result<Option<ReleaseInfo>, String> {
    let deadline = Instant::now() + CHECK_TIMEOUT;
    check_cancelled(cancelled, deadline)?;

    let agent = build_strict_agent(CHECK_TIMEOUT, CHECK_TIMEOUT);
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp = agent
        .get(&url)
        .header("User-Agent", format!("Ferric/{current_version}"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Network error while checking updates: {e}"))?;

    check_cancelled(cancelled, deadline)?;
    let status = resp.status().as_u16();
    if status == 404 {
        return Err("No published stable release was found on GitHub".to_string());
    }
    if status != 200 {
        return Err(format!("GitHub update check returned HTTP {status}"));
    }

    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_RELEASE_JSON_BYTES as u64)
        .read_to_vec()
        .map_err(|e| format!("Could not read GitHub release response: {e}"))?;

    check_cancelled(cancelled, deadline)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "Release JSON is not UTF-8".to_string())?;
    parse_latest_release(text, current_version)
}

fn open_verified_github_response(
    initial_url: &str,
    user_agent: &str,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(ureq::Body, Option<u64>), String> {
    let mut current_url = initial_url.to_owned();
    for _hop in 0..=4 {
        check_cancelled(cancelled, deadline)?;
        validate_https_download_url(&current_url)?;
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_secs(1));
        let phase = remaining.min(PHASE_TIMEOUT);
        let agent = build_strict_agent(remaining, phase);
        let resp = agent
            .get(&current_url)
            .header("User-Agent", user_agent)
            .call()
            .map_err(|e| format!("Download request failed: {e}"))?;

        let status = resp.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "Invalid update redirect header".to_string())?;
            current_url = location.to_owned();
            continue;
        }
        if status == 404 {
            return Err("Update package was not found on GitHub".to_string());
        }
        if status != 200 {
            return Err(format!("Update download returned HTTP {status}"));
        }
        let content_length = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        return Ok((resp.into_body(), content_length));
    }
    Err("Too many update download redirects".to_string())
}

/// Download `SHA256SUMS.txt` and `ferricsetup-{tag}.msi` exclusively from allowlisted GitHub hosts,
/// stream to an exclusively-created CSPRNG `.part` file, verify SHA-256, and atomically rename on success.
pub fn download_verified_installer(
    tag: &str,
    current_version: &str,
    updates_dir: &Path,
    cancelled: &AtomicBool,
    mut on_progress: impl FnMut(Option<u8>),
) -> Result<(PathBuf, [u8; 32]), String> {
    let canonical = normalize_tag(tag)?;
    let user_agent = format!("Ferric/{current_version}");
    let deadline = Instant::now() + DOWNLOAD_TIMEOUT;

    on_progress(Some(0));
    let sums_url =
        format!("https://github.com/{GITHUB_REPO}/releases/download/{canonical}/SHA256SUMS.txt");
    let (sums_body, _) =
        open_verified_github_response(&sums_url, &user_agent, cancelled, deadline)?;
    let sums_bytes = sums_body
        .into_with_config()
        .limit(MAX_CHECKSUMS_BYTES as u64)
        .read_to_vec()
        .map_err(|e| format!("Could not read SHA256SUMS.txt: {e}"))?;
    let sums_text =
        std::str::from_utf8(&sums_bytes).map_err(|_| "SHA256SUMS.txt is not UTF-8".to_string())?;
    let (installer_name, expected_sha) = resolve_installer_from_sums(sums_text, &canonical)?;

    let msi_url =
        format!("https://github.com/{GITHUB_REPO}/releases/download/{canonical}/{installer_name}");
    let (msi_body, total_bytes) =
        open_verified_github_response(&msi_url, &user_agent, cancelled, deadline)?;
    if total_bytes.is_some_and(|n| n > MAX_INSTALLER_BYTES) {
        return Err("Installer exceeds maximum allowed size".to_string());
    }

    fs::create_dir_all(updates_dir)
        .map_err(|e| format!("Could not prepare updates folder: {e}"))?;
    let final_path = updates_dir.join(&installer_name);
    let nonce = ferric_core::secrets::csprng_hex(8);
    let part_path = updates_dir.join(format!("{installer_name}.{nonce}.part"));

    let write_res: Result<(), String> = (|| {
        let mut reader = msi_body
            .into_with_config()
            .limit(MAX_INSTALLER_BYTES)
            .reader();
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part_path)
            .map_err(|e| format!("Could not create download file: {e}"))?;
        let mut writer = BufWriter::new(file);
        let mut downloaded = 0u64;
        let mut last_pct: Option<u8> = None;
        let mut buf = [0u8; 64 * 1024];

        loop {
            check_cancelled(cancelled, deadline)?;
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("Download read failed: {e}"))?;
            if n == 0 {
                break;
            }
            downloaded = downloaded
                .checked_add(n as u64)
                .ok_or_else(|| "Download size overflow".to_string())?;
            if downloaded > MAX_INSTALLER_BYTES {
                return Err("Installer download exceeded maximum allowed size".to_string());
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| format!("Could not write download file: {e}"))?;
            if let Some(total) = total_bytes.filter(|&t| t > 0) {
                let pct = ((downloaded.saturating_mul(100)) / total).min(99) as u8;
                if last_pct != Some(pct) {
                    last_pct = Some(pct);
                    on_progress(Some(pct));
                }
            } else if last_pct.is_none() {
                last_pct = Some(0);
                on_progress(None);
            }
        }
        writer
            .flush()
            .map_err(|e| format!("Could not flush download file: {e}"))?;
        if downloaded == 0 {
            return Err("Downloaded installer file was empty".to_string());
        }
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&part_path);
        return Err(err);
    }

    on_progress(Some(100));
    let actual_sha = match sha256_file(&part_path) {
        Ok(digest) => digest,
        Err(err) => {
            let _ = fs::remove_file(&part_path);
            return Err(err);
        }
    };
    if actual_sha != expected_sha {
        let _ = fs::remove_file(&part_path);
        return Err("Installer SHA-256 verification failed".to_string());
    }

    if final_path.exists() {
        let _ = fs::remove_file(&final_path);
    }
    if let Err(err) = fs::rename(&part_path, &final_path) {
        let _ = fs::remove_file(&part_path);
        return Err(format!("Could not finalize installer file: {err}"));
    }
    Ok((final_path, expected_sha))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_strict_numeric_triples() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert!(parse_version("0.10.0") > parse_version("0.9.99"));
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "01.2.3",
            "1.2.03",
            "1.2.3-alpha",
            "1.2.3+build",
            "1.-2.3",
            "1. 2.3",
        ] {
            assert_eq!(parse_version(bad), None, "unexpectedly accepted {bad}");
        }
    }

    #[test]
    fn release_parser_filters_drafts_prereleases_and_older_versions() {
        let newer = parse_latest_release(
            r###"{"tag_name":"v0.2.0","draft":false,"prerelease":false,"body":"## Added\n- Better Liquid Glass caustics"}"###,
            "0.1.0",
        )
        .expect("parse newer")
        .expect("should detect update");
        assert_eq!(newer.tag, "v0.2.0");
        assert_eq!(newer.version, "0.2.0");
        assert_eq!(
            newer.release_url,
            "https://github.com/Raflyf/Ferric/releases/tag/v0.2.0"
        );
        assert!(newer.summary.contains("Better Liquid Glass caustics"));

        assert_eq!(
            parse_latest_release(
                r#"{"tag_name":"v0.1.0","draft":false,"prerelease":false}"#,
                "0.1.0"
            )
            .expect("same version"),
            None
        );
        assert_eq!(
            parse_latest_release(
                r#"{"tag_name":"v0.9.0","draft":true,"prerelease":false}"#,
                "0.1.0"
            )
            .expect("draft ignored"),
            None
        );
        assert_eq!(
            parse_latest_release(
                r#"{"tag_name":"v0.9.0-beta","draft":false,"prerelease":true}"#,
                "0.1.0"
            )
            .expect("prerelease ignored"),
            None
        );
        for malformed in [
            "not json",
            "{}",
            r#"{"tag_name":"v01.2.0","draft":false,"prerelease":false}"#,
            r#"{"tag_name":"v0.2.0-rc1","draft":false,"prerelease":false}"#,
        ] {
            assert!(
                parse_latest_release(malformed, "0.1.0").is_err(),
                "accepted malformed: {malformed}"
            );
        }
    }

    #[test]
    fn checksum_and_url_validation_enforce_github_hosts_and_exact_sha256() {
        assert_eq!(
            installer_filename("v0.2.0").expect("msi name"),
            "ferricsetup-v0.2.0.msi"
        );
        assert!(installer_filename("../evil").is_err());

        assert!(
            validate_https_download_url(
                "https://github.com/Raflyf/Ferric/releases/download/v0.2.0/ferricsetup-v0.2.0.msi"
            )
            .is_ok()
        );
        assert!(
            validate_https_download_url(
                "https://objects.githubusercontent.com/github-production/ferric"
            )
            .is_ok()
        );
        assert!(
            validate_https_download_url(
                "https://release-assets.githubusercontent.com/github-production/ferric"
            )
            .is_ok()
        );
        assert!(validate_https_download_url("http://github.com/a").is_err());
        assert!(validate_https_download_url("https://evil.example/a.msi").is_err());
        assert!(validate_https_download_url("https://github.com@evil.example/a.msi").is_err());

        let expected = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let manifest = format!("{expected}  ferricsetup-v0.2.0.msi\n");
        let (name, sha) = resolve_installer_from_sums(&manifest, "v0.2.0").expect("resolve msi");
        assert_eq!(name, "ferricsetup-v0.2.0.msi");
        assert_eq!(sha[0], 0x01);
        assert_eq!(sha[31], 0xef);
        assert!(parse_sha256sums(&manifest, "other.msi").is_err());

        let tmp =
            std::env::temp_dir().join(format!("ferric-sha256-test-{}.bin", std::process::id()));
        fs::write(&tmp, b"abc").expect("write temp");
        let digest = sha256_file(&tmp).expect("hash file");
        let _ = fs::remove_file(&tmp);
        assert_eq!(
            digest,
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
    }

    #[test]
    fn cancelled_check_skips_network() {
        let err = check_latest_release("0.1.0", &AtomicBool::new(true)).unwrap_err();
        assert!(err.contains("cancelled"));
    }
}
