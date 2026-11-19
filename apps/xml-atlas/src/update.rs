//! Telling the user about a newer version (`docs/spec/install.md`, PKG-7; `docs/decisions.md`,
//! D30).
//!
//! At most once a day, a thread of its own asks GitHub for the latest published release and
//! sends the answer back through a channel; the status bar shows it on the next frame. Nothing
//! is downloaded or installed, and nothing is sent but that request: no cookie, no token, no
//! identifier, a `User-Agent` with the program's version only. Errors go to the log, except
//! after *Check for updates now*, which says what went wrong.

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

/// GitHub's latest published release; never a draft or a pre-release.
const LATEST: &str = "https://api.github.com/repos/glaure/xml-atlas-rs/releases/latest";
/// Where a notice links when the answer has no usable link of its own.
pub const RELEASES: &str = "https://github.com/glaure/xml-atlas-rs/releases";
/// The program's own version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Seconds between two automatic checks.
const DAY: u64 = 24 * 60 * 60;
/// The whole request, from connecting to the last byte.
const TIMEOUT: Duration = Duration::from_secs(10);
/// GitHub's answer is about 10 KB with the release notes; more than this is not one.
const ANSWER_LIMIT: u64 = 1024 * 1024;

/// A version as three numbers, `1.10.0` after `1.9.2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

/// The latest release, as GitHub describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The release's page.
    pub url: String,
}

/// How the program was installed: it decides what a notice asks the user to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    /// The MSI, the zip or the DMG: the notice links to the release.
    Download,
    /// A `.deb` or `.rpm`: update with the package manager.
    Package,
    /// The Flatpak: update with the software centre or `flatpak update`.
    Flatpak,
}

/// What the status bar says after a check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A newer release exists.
    Newer(Release, Install),
    /// After *Check now*: this version is the latest.
    UpToDate,
    /// After *Check now*: the check failed.
    Failed(String),
}

/// The request under way, if any.
#[derive(Default)]
pub struct Checker {
    running: Option<Running>,
}

struct Running {
    rx: Receiver<Result<Release, String>>,
    /// Started by *Check now*, rather than by the daily check.
    asked: bool,
}

impl Version {
    /// `1.2.3` or `v1.2.3`; anything else, a pre-release such as `1.2.0-rc.1` too, is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.split('.').map(|p| {
            // `u64::from_str` takes a leading `+`; a version does not have one.
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                p.parse().ok()
            }
        });
        let v = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl Install {
    /// How this program was installed: the Flatpak sets `FLATPAK_ID`; the packages put it into
    /// `/usr/bin`.
    pub fn detect() -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        Self::of(std::env::var_os("FLATPAK_ID").is_some(), &exe)
    }

    fn of(flatpak: bool, exe: &Path) -> Self {
        if flatpak {
            Install::Flatpak
        } else if cfg!(target_os = "linux") && exe.starts_with("/usr/bin") {
            Install::Package
        } else {
            Install::Download
        }
    }
}

impl Notice {
    /// The status bar's words.
    pub fn text(&self) -> String {
        match self {
            Notice::Newer(r, Install::Download) => format!("XML Atlas {} is available", r.version),
            Notice::Newer(r, Install::Package) => format!(
                "XML Atlas {} is available: update it with your package manager",
                r.version
            ),
            Notice::Newer(r, Install::Flatpak) => format!(
                "XML Atlas {} is available: update it with your software centre or \
                 `flatpak update`",
                r.version
            ),
            Notice::UpToDate => format!("XML Atlas {CURRENT} is the latest version"),
            Notice::Failed(e) => format!("Cannot check for updates: {e}"),
        }
    }

    /// The release page to link to: only for a download, as a package is updated elsewhere.
    pub fn link(&self) -> Option<&str> {
        match self {
            Notice::Newer(r, Install::Download) => Some(&r.url),
            _ => None,
        }
    }
}

impl Checker {
    /// Asks GitHub on a thread of its own; `repaint` wakes the window for the answer. `asked`
    /// is set for *Check now*. Does nothing while a request is under way.
    pub fn start(&mut self, repaint: egui::Context, asked: bool) {
        if let Some(r) = &mut self.running {
            r.asked |= asked;
            return;
        }
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let answer = fetch();
                // The window may be gone; then nobody waits for the answer.
                let _ = tx.send(answer);
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => self.running = Some(Running { rx, asked }),
            Err(e) => log::warn!("update check: cannot start a thread: {e}"),
        }
    }

    /// The answer when it has come: whether *Check now* asked, and what came.
    pub fn poll(&mut self) -> Option<(bool, Result<Release, String>)> {
        let r = self.running.as_ref()?;
        let answer = match r.rx.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the check stopped".into()),
        };
        let asked = r.asked;
        self.running = None;
        Some((asked, answer))
    }

    pub fn running(&self) -> bool {
        self.running.is_some()
    }
}

/// Whether the daily check is due: it is on, and the last one was a day ago or more, or never,
/// or in the future (the clock was set back).
pub fn due(on: bool, last: Option<u64>, now: u64) -> bool {
    on && last.is_none_or(|last| now >= last.saturating_add(DAY) || now < last)
}

/// What to tell the user about `answer`, given the version the notice was last shown for.
/// The daily check says only that a version is newer, and once per version; *Check now*
/// (`asked`) always says what it found.
pub fn notice(
    asked: bool,
    answer: Result<Release, String>,
    current: Version,
    shown: Option<Version>,
    install: Install,
) -> Option<Notice> {
    if let Ok(r) = &answer {
        log::info!("update check: the latest release is {}", r.version);
    }
    match answer {
        Ok(r) if r.version > current => {
            (asked || shown != Some(r.version)).then_some(Notice::Newer(r, install))
        }
        Ok(_) => asked.then_some(Notice::UpToDate),
        Err(e) => {
            log::info!("update check: {e}");
            asked.then_some(Notice::Failed(e))
        }
    }
}

/// When the last check was, in words for the Settings window.
pub fn ago(last: Option<u64>, now: u64) -> String {
    let Some(last) = last else {
        return "never".into();
    };
    let s = now.saturating_sub(last);
    match s / 3600 {
        0 => "less than an hour ago".into(),
        1 => "an hour ago".into(),
        h @ 2..24 => format!("{h} hours ago"),
        24..48 => "a day ago".into(),
        h => format!("{} days ago", h / 24),
    }
}

/// Seconds since 1970, the clock of [`due`] and [`ago`].
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Reads GitHub's answer: `tag_name` is the version, `html_url` the page. A link that is not a
/// page of this project's releases is replaced by [`RELEASES`].
pub fn read_release(answer: &[u8]) -> Result<Release, String> {
    let v: serde_json::Value =
        serde_json::from_slice(answer).map_err(|e| format!("cannot read the answer: {e}"))?;
    let tag = v["tag_name"]
        .as_str()
        .ok_or("the answer names no version")?;
    let version = Version::parse(tag).ok_or_else(|| format!("{tag:?} is not a version"))?;
    let url = v["html_url"]
        .as_str()
        .filter(|u| u.starts_with(RELEASES) && !u.contains(char::is_whitespace))
        .unwrap_or(RELEASES)
        .to_string();
    Ok(Release { version, url })
}

/// The request itself. Certificates are checked against the system's store, so a proxy that
/// inspects TLS with the company's own certificate works as in the browser.
fn fetch() -> Result<Release, String> {
    use ureq::tls::{RootCerts, TlsConfig};
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(format!("xml-atlas/{CURRENT}"))
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent();
    let mut response = agent
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let body = response
        .body_mut()
        .with_config()
        .limit(ANSWER_LIMIT)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    read_release(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &[u8] = include_bytes!("../tests/data/latest-release.json");

    fn release(v: Version) -> Release {
        Release {
            version: v,
            url: format!("{RELEASES}/tag/v{v}"),
        }
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(Version::parse("v1.10.0"), Some(Version(1, 10, 0)));
        assert!(Version::parse("v1.10.0") > Version::parse("v1.9.2"));
        assert_eq!(Version::parse("v1.0.0"), Version::parse("1.0.0"));
        assert!(Version::parse(CURRENT).is_some(), "{CURRENT}");
        for bad in [
            "",
            "v",
            "1.0",
            "1.0.0.0",
            "1.2.0-rc.1",
            "v1..0",
            "1.+2.0",
            "x1.0.0",
            "1.0.0 ",
            "１.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn githubs_answer_is_read() {
        assert_eq!(
            read_release(ANSWER),
            Ok(Release {
                version: Version(1, 1, 0),
                url: "https://github.com/glaure/xml-atlas-rs/releases/tag/v1.1.0".into(),
            })
        );
        // A tag that is not a version is no release; a foreign link is not followed.
        assert!(read_release(br#"{"tag_name":"nightly"}"#).is_err());
        assert!(read_release(b"<html>").is_err());
        assert!(read_release(br#"{"message":"API rate limit exceeded"}"#).is_err());
        assert_eq!(
            read_release(br#"{"tag_name":"v2.0.0","html_url":"https://example.com/"}"#)
                .unwrap()
                .url,
            RELEASES
        );
    }

    #[test]
    fn once_a_day() {
        let now = 1_800_000_000;
        assert!(due(true, None, now));
        assert!(!due(false, None, now));
        assert!(!due(true, Some(now - DAY + 1), now));
        assert!(due(true, Some(now - DAY), now));
        assert!(!due(false, Some(now - 10 * DAY), now));
        // The clock was set back: the time noted cannot be trusted.
        assert!(due(true, Some(now + 60), now));
    }

    #[test]
    fn once_per_version_unless_asked() {
        let current = Version(1, 1, 0);
        let newer = || Ok(release(Version(1, 2, 0)));
        let d = Install::Download;
        let n = |asked, answer, shown| notice(asked, answer, current, shown, d);
        assert_eq!(
            n(false, newer(), None),
            Some(Notice::Newer(release(Version(1, 2, 0)), d))
        );
        assert_eq!(n(false, newer(), Some(Version(1, 2, 0))), None);
        assert!(n(false, newer(), Some(Version(1, 1, 5))).is_some());
        assert!(n(true, newer(), Some(Version(1, 2, 0))).is_some());
        // The same version, or an older one: silent, unless asked.
        assert_eq!(n(false, Ok(release(current)), None), None);
        assert_eq!(n(false, Ok(release(Version(1, 0, 9))), None), None);
        assert_eq!(n(true, Ok(release(current)), None), Some(Notice::UpToDate));
        // No network: silent, unless asked.
        assert_eq!(n(false, Err("offline".into()), None), None);
        assert_eq!(
            n(true, Err("offline".into()), None),
            Some(Notice::Failed("offline".into()))
        );
    }

    #[test]
    fn packages_are_updated_by_their_manager() {
        let r = release(Version(1, 2, 0));
        assert_eq!(
            Install::of(true, Path::new("/app/bin/xml-atlas")),
            Install::Flatpak
        );
        let packaged = Install::of(false, Path::new("/usr/bin/xml-atlas"));
        let downloaded = Install::of(false, Path::new("/opt/xml-atlas/xml-atlas"));
        assert_eq!(downloaded, Install::Download);
        if cfg!(target_os = "linux") {
            assert_eq!(packaged, Install::Package);
        } else {
            assert_eq!(packaged, Install::Download);
        }
        let n = Notice::Newer(r.clone(), Install::Download);
        assert_eq!(n.text(), "XML Atlas 1.2.0 is available");
        assert_eq!(n.link(), Some(r.url.as_str()));
        let n = Notice::Newer(r.clone(), Install::Package);
        assert!(n.text().contains("package manager"));
        assert_eq!(n.link(), None);
        assert!(
            Notice::Newer(r, Install::Flatpak)
                .text()
                .contains("flatpak update")
        );
    }

    #[test]
    fn last_check_in_words() {
        let now = 1_800_000_000;
        assert_eq!(ago(None, now), "never");
        assert_eq!(ago(Some(now - 59 * 60), now), "less than an hour ago");
        assert_eq!(ago(Some(now - 3600), now), "an hour ago");
        assert_eq!(ago(Some(now - 5 * 3600), now), "5 hours ago");
        assert_eq!(ago(Some(now - DAY), now), "a day ago");
        assert_eq!(ago(Some(now - 3 * DAY), now), "3 days ago");
        assert_eq!(ago(Some(now + 60), now), "less than an hour ago");
    }
}
