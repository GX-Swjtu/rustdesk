use base::config::keys;
use hbb_common::{
    config::{self, LocalConfig},
    log,
    tls::TlsType,
    tokio, ResultType,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Mutex,
    },
    time::Duration,
};

pub const REPOSITORY: &str = "GX-Swjtu/rustdesk";
const UPSTREAM: &str = "rustdesk/rustdesk";
const BUILD_VERSION: &str = include_str!("../res/fork-version");
const UPSTREAM_BASE: &str = include_str!("../res/fork-upstream-version");

pub fn enabled() -> bool {
    cfg!(feature = "fork-updates")
}

pub fn version() -> &'static str {
    BUILD_VERSION.trim()
}

#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub current: String,
    pub upstream: String,
    pub upstream_newer: bool,
    pub available: String,
    pub url: String,
    pub error: bool,
}

lazy_static::lazy_static! {
    static ref STATUS: Mutex<Status> = Mutex::new(Status::default());
    static ref CHECK_REQUESTS: SyncSender<()> = {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || check_worker(rx));
        tx
    };
}

pub fn request_check() {
    match CHECK_REQUESTS.try_send(()) {
        Ok(()) | Err(TrySendError::Full(())) => {}
        Err(TrySendError::Disconnected(())) => log::error!("Fork update worker stopped"),
    }
}

// Started only by the GUI's update entry point, so results reach its event stream.
// A bounded queue coalesces repeated clicks while a check is running.
fn check_worker(requests: Receiver<()>) {
    let mut interval = Duration::from_secs(60 * 60);
    loop {
        match requests.recv_timeout(interval) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let option = LocalConfig::get_option(keys::OPTION_ENABLE_CHECK_UPDATE);
        if !config::option2bool(keys::OPTION_ENABLE_CHECK_UPDATE, &option) {
            continue;
        }
        interval = match crate::common::do_check_software_update() {
            Ok(()) => Duration::from_secs(60 * 60),
            Err(err) => {
                log::warn!("Fork update check failed: {err}");
                Duration::from_secs(15 * 60)
            }
        };
    }
}

pub fn status_json() -> String {
    if !enabled() {
        return String::new();
    }
    match serde_json::to_string(&*STATUS.lock().unwrap()) {
        Ok(value) => value,
        Err(err) => {
            log::error!("Cannot serialize fork update status: {err}");
            String::new()
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u16, u16, u16, u16);

fn parse_version(value: &str, fork: bool) -> Option<Version> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let (base, revision) = if fork {
        let (base, revision) = value.split_once("-gx.")?;
        (base, revision.parse().ok()?)
    } else {
        (value, 0)
    };
    let parts: Vec<_> = base.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some(Version(
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        revision,
    ))
}

#[derive(Default, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
}

fn published_version(release: &Release, fork: bool) -> Option<Version> {
    if release.draft || release.prerelease || release.published_at.is_none() {
        return None;
    }
    parse_version(&release.tag_name, fork)
}

fn update_url(release: &Release, current: &str, platform: &str, arch: &str) -> Option<String> {
    if published_version(release, true)? <= parse_version(current, true)? {
        return None;
    }
    let prefix = format!(
        "https://github.com/{REPOSITORY}/releases/download/{}/",
        release.tag_name
    );
    let valid_asset = |asset: &&Asset| {
        asset.size > 0 && asset.browser_download_url == format!("{prefix}{}", asset.name)
    };
    if !release
        .assets
        .iter()
        .filter(valid_asset)
        .any(|a| a.name == "fork-release.json")
    {
        return None;
    }
    let suffix = match (platform, arch) {
        ("windows", "x86") => "-x86-sciter.exe".to_owned(),
        ("windows", "x86_64" | "aarch64") => format!("-{arch}.exe"),
        ("macos", "x86_64" | "aarch64") => format!("-{arch}.dmg"),
        ("linux", "x86_64" | "aarch64") => format!("-{arch}.deb"),
        ("linux", "arm") => "-armv7-sciter.deb".to_owned(),
        ("android", _) => "-universal.apk".to_owned(),
        _ => return None,
    };
    let expected = format!("rustdesk-{}{suffix}", release.tag_name);
    if !release
        .assets
        .iter()
        .filter(valid_asset)
        .any(|a| a.name == expected)
    {
        return None;
    }
    Some(format!(
        "https://github.com/{REPOSITORY}/releases/tag/{}",
        release.tag_name
    ))
}

async fn latest(client: &reqwest::Client, repository: &str) -> ResultType<Option<Release>> {
    let response = client
        .get(format!(
            "https://api.github.com/repos/{repository}/releases/latest"
        ))
        .header("User-Agent", "RustDesk-GX-update-check")
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    Ok(Some(response.error_for_status()?.json().await?))
}

pub async fn check() -> ResultType<()> {
    let client = crate::hbbs_http::create_http_client_async(TlsType::Rustls, false);
    let (upstream, fork) = tokio::join!(latest(&client, UPSTREAM), latest(&client, REPOSITORY));
    let mut status = Status {
        current: version().to_owned(),
        ..Default::default()
    };
    match upstream {
        Ok(Some(release)) => {
            if let Some(latest) = published_version(&release, false) {
                let base = UPSTREAM_BASE.trim();
                status.upstream_newer = parse_version(base, false)
                    .map(|v| latest > v)
                    .unwrap_or(false);
                status.upstream = release.tag_name;
            }
        }
        Ok(None) => {}
        Err(err) => {
            status.error = true;
            log::warn!("Upstream release check failed: {err}");
        }
    }
    match fork {
        Ok(Some(release)) => {
            if let Some(url) = update_url(
                &release,
                version(),
                std::env::consts::OS,
                std::env::consts::ARCH,
            ) {
                status.available = release.tag_name;
                status.url = url;
            }
        }
        Ok(None) => {}
        Err(err) => {
            status.error = true;
            log::warn!("Fork release check failed: {err}");
        }
    }
    // The official installer must never consume a fork release or an upstream notification.
    crate::common::SOFTWARE_UPDATE_URL.lock().unwrap().clear();
    *STATUS.lock().unwrap() = status.clone();
    #[cfg(feature = "flutter")]
    {
        let event =
            serde_json::json!({"name": "check_software_update_finish", "url": "", "fork": status});
        let _ = crate::flutter::push_global_event(crate::flutter::APP_TYPE_MAIN, event.to_string());
    }
    if status.error {
        hbb_common::bail!("GitHub release check incomplete");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        let assets = [
            "fork-release.json".to_owned(),
            format!("rustdesk-{tag}-x86_64.exe"),
        ]
        .into_iter()
        .map(|name| Asset {
            browser_download_url: format!(
                "https://github.com/{REPOSITORY}/releases/download/{tag}/{name}"
            ),
            name,
            size: 10,
        })
        .collect();
        Release {
            tag_name: tag.to_owned(),
            published_at: Some("2026-10-15".to_owned()),
            assets,
            ..Default::default()
        }
    }

    #[test]
    fn compares_upstream_and_fork_revisions_numerically() {
        assert!(parse_version("1.5.1-gx.10", true) > parse_version("1.5.1-gx.9", true));
        assert!(parse_version("1.5.2-gx.1", true) > parse_version("1.5.1-gx.99", true));
        assert_eq!(parse_version("nightly", false), None);
        assert_eq!(parse_version("1.5.2-rc.1", false), None);
        assert_eq!(parse_version("1.5.2", true), None);
    }

    #[test]
    fn only_complete_published_newer_fork_releases_are_downloadable() {
        let mut r = release("1.5.1-gx.2");
        assert_eq!(
            update_url(&r, "1.5.1-gx.1", "windows", "x86_64"),
            Some(format!(
                "https://github.com/{REPOSITORY}/releases/tag/1.5.1-gx.2"
            ))
        );
        assert!(update_url(&r, "1.5.1-gx.2", "windows", "x86_64").is_none());
        assert!(update_url(&r, "1.5.2-gx.1", "windows", "x86_64").is_none());
        assert!(update_url(&r, "1.5.1-gx.1", "macos", "aarch64").is_none());
        r.draft = true;
        assert!(update_url(&r, "1.5.1-gx.1", "windows", "x86_64").is_none());
        r.draft = false;
        r.prerelease = true;
        assert!(update_url(&r, "1.5.1-gx.1", "windows", "x86_64").is_none());
        r.prerelease = false;
        r.assets.remove(0);
        assert!(update_url(&r, "1.5.1-gx.1", "windows", "x86_64").is_none());
    }

    #[test]
    fn upstream_assets_and_forged_download_urls_cannot_enable_updates() {
        let mut r = release("1.5.1-gx.2");
        r.assets[1].browser_download_url =
            "https://github.com/rustdesk/rustdesk/releases/download/1.5.1/rustdesk.exe".into();
        assert!(update_url(&r, "1.5.1-gx.1", "windows", "x86_64").is_none());
        r.tag_name = "1.6.0".into();
        assert!(update_url(&r, "1.5.1-gx.1", "windows", "x86_64").is_none());
    }
}
