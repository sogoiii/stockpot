//! Version check functionality for checking GitHub releases.
//!
//! This module provides functionality to:
//! - Fetch the latest release from GitHub
//! - Compare versions using semver
//! - Determine if an update is available

use anyhow::{Context, Result};
use serde::Deserialize;

/// GitHub API endpoint for latest release
const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/fed-stew/stockpot/releases/latest";

/// Current application version (from Cargo.toml)
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Information about a GitHub release
#[derive(Debug, Clone)]
pub struct LatestRelease {
    /// Version string without 'v' prefix (e.g., "0.6.0")
    pub version: String,
    /// Original tag name (e.g., "v0.6.0")
    pub tag_name: String,
    /// URL to the release page
    pub html_url: String,
}

/// GitHub API response structure
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

/// Fetches the latest release information from GitHub.
///
/// Returns `None` if the request fails or the response is invalid.
/// This is designed to fail silently - version checks should not
/// disrupt the user experience.
pub async fn fetch_latest_release() -> Result<LatestRelease> {
    let client = reqwest::Client::new();
    let response = client
        .get(GITHUB_RELEASES_URL)
        .header("User-Agent", "stockpot-cli")
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .context("Failed to fetch releases from GitHub")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub API returned status {}", response.status());
    }

    let release: GitHubRelease = response
        .json()
        .await
        .context("Failed to parse GitHub release response")?;

    // Strip 'v' prefix from tag name if present
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();

    Ok(LatestRelease {
        version,
        tag_name: release.tag_name,
        html_url: release.html_url,
    })
}

/// Compares two version strings and returns true if `latest` is newer than `current`.
///
/// Uses semver parsing for accurate comparison. Returns `false` if either
/// version string is invalid (fail-safe behavior).
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (
        semver::Version::parse(current),
        semver::Version::parse(latest),
    ) {
        (Ok(curr), Ok(lat)) => lat > curr,
        _ => {
            tracing::warn!(
                "Failed to parse versions for comparison: current={}, latest={}",
                current,
                latest
            );
            false
        }
    }
}

/// Checks if an update is available and returns the release info if so.
///
/// This function:
/// 1. Fetches the latest release from GitHub
/// 2. Compares it to the current version
/// 3. Returns the release info only if a newer version exists
///
/// Returns `None` if:
/// - The fetch fails (network error, API error, etc.)
/// - The current version is up to date
pub async fn check_for_update() -> Option<LatestRelease> {
    match fetch_latest_release().await {
        Ok(release) => {
            if is_newer_version(CURRENT_VERSION, &release.version) {
                tracing::debug!(
                    "Update available: {} -> {}",
                    CURRENT_VERSION,
                    release.version
                );
                Some(release)
            } else {
                tracing::debug!(
                    "No update available: current={}, latest={}",
                    CURRENT_VERSION,
                    release.version
                );
                None
            }
        }
        Err(e) => {
            tracing::debug!("Failed to check for updates: {}", e);
            None
        }
    }
}

/// Print update notification to terminal if a newer version is available.
///
/// This is a convenience function for CLI usage that handles the async
/// check and prints a formatted message.
pub fn print_update_notification_blocking() {
    // Run the async check in a blocking context
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            // We're already in a tokio runtime, spawn a blocking task
            std::thread::spawn(move || {
                handle.block_on(async {
                    if let Some(release) = check_for_update().await {
                        print_update_message(&release);
                    }
                })
            });
            return;
        }
        Err(_) => {
            // No runtime, create a new one
            match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(_) => return,
            }
        }
    };

    rt.block_on(async {
        if let Some(release) = check_for_update().await {
            print_update_message(&release);
        }
    });
}

/// Print the update message to stderr
pub fn print_update_message(release: &LatestRelease) {
    use nu_ansi_term::{Color, Style};

    let yellow = Style::new().fg(Color::Yellow).bold();
    let cyan = Style::new().fg(Color::Cyan);
    let dim = Style::new().dimmed();

    eprintln!();
    eprintln!(
        "{} A new version of stockpot is available: {} → {}",
        yellow.paint("⬆"),
        dim.paint(CURRENT_VERSION),
        cyan.paint(&release.version)
    );
    eprintln!("  {}", dim.paint(&release.html_url));
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.5.0", "0.6.0"));
        assert!(is_newer_version("0.5.0", "1.0.0"));
        assert!(is_newer_version("0.5.0", "0.5.1"));
        assert!(!is_newer_version("0.6.0", "0.5.0"));
        assert!(!is_newer_version("0.5.0", "0.5.0"));
        assert!(!is_newer_version("1.0.0", "0.99.99"));
    }

    #[test]
    fn test_is_newer_version_invalid() {
        // Invalid versions should return false
        assert!(!is_newer_version("invalid", "0.6.0"));
        assert!(!is_newer_version("0.5.0", "invalid"));
        assert!(!is_newer_version("invalid", "also-invalid"));
    }

    #[test]
    fn test_current_version_is_valid() {
        // Ensure the embedded version is a valid semver
        let version = semver::Version::parse(CURRENT_VERSION);
        assert!(
            version.is_ok(),
            "CURRENT_VERSION '{}' should be valid semver",
            CURRENT_VERSION
        );
    }
}
