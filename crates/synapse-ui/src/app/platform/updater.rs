use std::sync::Arc;

use futures::AsyncReadExt as _;
use gpui::http_client::HttpClient;

pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/08820048/Synapse/releases/latest";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct AppVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UpdatePlatform {
    Macos,
    Windows,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvailableUpdate {
    pub version: String,
    pub release_url: String,
    pub download_url: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum UpdateCheckState {
    #[default]
    Idle,
    Checking,
    Available(AvailableUpdate),
    Current,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UpdateCheckOrigin {
    Startup,
    Manual,
}

impl AppVersion {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let value = value.trim().trim_start_matches(['v', 'V']);
        let value = value.split(['-', '+']).next()?;
        let mut parts = value.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    pub(crate) fn current() -> Option<Self> {
        Self::parse(APP_VERSION)
    }
}

pub(crate) fn current_update_platform() -> UpdatePlatform {
    if cfg!(target_os = "macos") {
        UpdatePlatform::Macos
    } else if cfg!(target_os = "windows") {
        UpdatePlatform::Windows
    } else {
        UpdatePlatform::Other
    }
}

pub(crate) fn preferred_download_url(
    assets: &[(String, String)],
    platform: UpdatePlatform,
) -> Option<&str> {
    let expected_suffix = match platform {
        UpdatePlatform::Macos => ".dmg",
        UpdatePlatform::Windows => ".exe",
        UpdatePlatform::Other => return None,
    };
    let expected_marker = match platform {
        UpdatePlatform::Macos => "macos",
        UpdatePlatform::Windows => "windows",
        UpdatePlatform::Other => return None,
    };
    assets.iter().find_map(|(name, url)| {
        let lower = name.to_ascii_lowercase();
        (lower.ends_with(expected_suffix) && lower.contains(expected_marker))
            .then_some(url.as_str())
    })
}

pub(crate) fn should_prompt_for_update(
    latest: &AvailableUpdate,
    dismissed_version: Option<&str>,
) -> bool {
    dismissed_version != Some(latest.version.as_str())
}

pub(crate) fn parse_latest_release(
    body: &str,
    platform: UpdatePlatform,
) -> Result<AvailableUpdate, String> {
    let tag = json_string_field(body, "tag_name").ok_or_else(|| "missing tag_name".to_owned())?;
    let version = AppVersion::parse(&tag).ok_or_else(|| format!("invalid tag {tag}"))?;
    let release_url = json_string_field(body, "html_url")
        .unwrap_or_else(|| format!("https://github.com/08820048/Synapse/releases/tag/{tag}"));
    let assets = json_named_assets(body);
    let download_url = preferred_download_url(&assets, platform)
        .unwrap_or(release_url.as_str())
        .to_owned();
    Ok(AvailableUpdate {
        version: format!("{}.{}.{}", version.major, version.minor, version.patch),
        release_url,
        download_url,
    })
}

pub(crate) fn classify_release(
    latest: AvailableUpdate,
    current: AppVersion,
) -> Result<AvailableUpdate, UpdateCheckState> {
    let Some(latest_version) = AppVersion::parse(&latest.version) else {
        return Err(UpdateCheckState::Failed(
            "invalid latest version".to_owned(),
        ));
    };
    if latest_version > current {
        Ok(latest)
    } else {
        Err(UpdateCheckState::Current)
    }
}

pub(crate) async fn fetch_latest_release(
    client: Arc<dyn HttpClient>,
    platform: UpdatePlatform,
) -> Result<AvailableUpdate, String> {
    let mut response = client
        .get(GITHUB_LATEST_RELEASE_URL, ().into(), true)
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    if status == 404 {
        return Err("no published release".to_owned());
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }

    let mut body = String::new();
    response
        .body_mut()
        .take(512 * 1024)
        .read_to_string(&mut body)
        .await
        .map_err(|error| error.to_string())?;
    parse_latest_release(&body, platform)
}

fn json_string_field(source: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = source.find(&needle)?;
    let after_key = &source[start + needle.len()..];
    let after_colon = after_key.split_once(':')?.1.trim_start();
    parse_json_string(after_colon)
}

fn parse_json_string(source: &str) -> Option<String> {
    let mut chars = source.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut output = String::new();
    let mut escaped = false;
    for character in chars {
        if escaped {
            output.push(match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '"' => return Some(output),
            other => output.push(other),
        }
    }
    None
}

fn json_named_assets(source: &str) -> Vec<(String, String)> {
    let Some(assets_key) = source.find("\"assets\"") else {
        return Vec::new();
    };
    let after_key = &source[assets_key + "\"assets\"".len()..];
    let Some(array_start) = after_key.find('[') else {
        return Vec::new();
    };
    let array = &after_key[array_start..];
    let mut assets = Vec::new();
    let mut search_from = 0;
    while let Some(relative) = array[search_from..].find("\"name\"") {
        let slice = &array[search_from + relative..];
        let Some(name) = json_string_field(slice, "name") else {
            break;
        };
        let Some(url) = json_string_field(slice, "browser_download_url") else {
            search_from += relative + 6;
            continue;
        };
        assets.push((name, url));
        search_from += relative + 6;
    }
    assets
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RELEASE: &str = r#"{
  "tag_name": "v0.2.0",
  "html_url": "https://github.com/08820048/Synapse/releases/tag/v0.2.0",
  "assets": [
    {
      "name": "Synapse-0.2.0-macos-universal.dmg",
      "url": "https://api.github.com/repos/08820048/Synapse/releases/assets/1",
      "browser_download_url": "https://github.com/08820048/Synapse/releases/download/v0.2.0/Synapse-0.2.0-macos-universal.dmg"
    },
    {
      "name": "Synapse-0.2.0-windows-x64.exe",
      "browser_download_url": "https://github.com/08820048/Synapse/releases/download/v0.2.0/Synapse-0.2.0-windows-x64.exe"
    }
  ]
}"#;

    #[test]
    fn parses_prefixed_and_plain_versions() {
        assert_eq!(
            AppVersion::parse("v0.1.2"),
            Some(AppVersion {
                major: 0,
                minor: 1,
                patch: 2
            })
        );
        assert_eq!(AppVersion::parse("0.1.2"), AppVersion::parse("v0.1.2"));
        assert_eq!(
            AppVersion::parse("1.0"),
            Some(AppVersion {
                major: 1,
                minor: 0,
                patch: 0
            })
        );
        assert!(AppVersion::parse("nope").is_none());
    }

    #[test]
    fn newer_releases_sort_after_the_running_version() {
        let current = AppVersion::parse("0.1.2").unwrap();
        let latest = AppVersion::parse("0.2.0").unwrap();
        assert!(latest > current);
        assert!(AppVersion::parse("0.1.2").unwrap() == current);
    }

    #[test]
    fn selects_platform_installers_and_ignores_api_asset_urls() {
        let release = parse_latest_release(SAMPLE_RELEASE, UpdatePlatform::Macos).unwrap();
        assert_eq!(release.version, "0.2.0");
        assert!(release.download_url.ends_with(".dmg"));
        assert!(!release.download_url.contains("/assets/"));

        let windows = parse_latest_release(SAMPLE_RELEASE, UpdatePlatform::Windows).unwrap();
        assert!(windows.download_url.ends_with(".exe"));
    }

    #[test]
    fn classify_release_only_prompts_when_newer() {
        let latest = parse_latest_release(SAMPLE_RELEASE, UpdatePlatform::Macos).unwrap();
        let current = AppVersion::parse("0.1.2").unwrap();
        assert!(classify_release(latest.clone(), current).is_ok());
        assert_eq!(
            classify_release(latest, AppVersion::parse("0.2.0").unwrap()),
            Err(UpdateCheckState::Current)
        );
    }

    #[test]
    fn dismissed_versions_suppress_the_startup_prompt() {
        let latest = AvailableUpdate {
            version: "0.2.0".to_owned(),
            release_url: "https://example.com".to_owned(),
            download_url: "https://example.com/app.dmg".to_owned(),
        };
        assert!(should_prompt_for_update(&latest, None));
        assert!(!should_prompt_for_update(&latest, Some("0.2.0")));
        assert!(should_prompt_for_update(&latest, Some("0.1.9")));
    }
}
