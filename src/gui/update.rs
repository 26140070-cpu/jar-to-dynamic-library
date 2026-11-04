use std::time::Duration;

use serde::Deserialize;

use crate::gui::state::GITHUB_URL;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/26140070-cpu/jar-to-dynamic-library/releases/latest";

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub release_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

pub fn check_for_update() -> Result<Option<UpdateInfo>, String> {
    let response = ureq::get(LATEST_RELEASE_API)
        .set("User-Agent", "JAR-to-Dynamic-Library")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|error| format!("GitHub release request failed: {error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("Could not read the GitHub release response: {error}"))?;
    let release: Release = serde_json::from_str(&body)
        .map_err(|error| format!("Could not parse the GitHub release response: {error}"))?;

    if release.draft || release.prerelease {
        return Ok(None);
    }

    let current_version = env!("CARGO_PKG_VERSION");
    if !is_newer_version(&release.tag_name, current_version)? {
        return Ok(None);
    }
    if !release.html_url.starts_with(&format!("{GITHUB_URL}/releases/")) {
        return Err("GitHub returned an unexpected release URL.".to_string());
    }

    Ok(Some(UpdateInfo {
        version: release.tag_name,
        release_url: release.html_url,
    }))
}

fn is_newer_version(latest: &str, current: &str) -> Result<bool, String> {
    Ok(parse_version(latest)? > parse_version(current)?)
}

fn parse_version(version: &str) -> Result<[u64; 3], String> {
    let version = version
        .trim()
        .strip_prefix('v')
        .unwrap_or(version.trim())
        .split(['-', '+'])
        .next()
        .unwrap_or_default();
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("Invalid release version: {version}"));
    }
    let mut parsed = [0; 3];
    for (index, part) in parts.iter().enumerate() {
        parsed[index] = part
            .parse()
            .map_err(|_| format!("Invalid release version: {version}"))?;
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{is_newer_version, parse_version};

    #[test]
    fn compares_release_versions_and_accepts_v_prefix() {
        assert!(is_newer_version("v2.1.0", "2.0.9").unwrap());
        assert!(is_newer_version("0.2.0", "0.1.0").unwrap());
        assert!(!is_newer_version("0.2.0", "0.2.1").unwrap());
        assert!(!is_newer_version("0.2.1", "0.2.1").unwrap());
    }

    #[test]
    fn ignores_prerelease_suffix_for_version_ordering() {
        assert_eq!(parse_version("2.1.0-rc.1").unwrap(), [2, 1, 0]);
    }

    #[test]
    fn rejects_non_semver_release_tags() {
        assert!(parse_version("latest").is_err());
    }
}
