//! Update check: at startup, at most once a day, asks GitHub for the latest release
//! (background thread, result polled every frame). Notify only: nothing is downloaded.
//! Every failure (offline, timeout, rate limit, private repository, unexpected JSON) is silent.

use std::cmp::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const LATEST_URL: &str = "https://api.github.com/repos/gurzil-io/sequensss/releases/latest";
/// Running version. Test builds set `SEQUENSSS_VERSION` (e.g. `0.1.5-test.1`) at build time.
pub const CURRENT: &str = match option_env!("SEQUENSSS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
const DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// Version without the `v` prefix (`0.1.4`).
    pub version: String,
    /// Release page (notes and downloads).
    pub url: String,
}

/// Semantic version: `major.minor.patch` plus an optional pre-release suffix (`-beta.2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    nums: [u64; 3],
    pre: Option<String>,
}

impl Version {
    /// `v0.1.10`, `0.1.10`, `1.0.0-beta`; build metadata (`+…`) is ignored.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let s = s.split_once('+').map_or(s, |(v, _)| v);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut nums = [0u64; 3];
        let mut parts = core.split('.');
        for n in &mut nums {
            *n = parts.next()?.parse().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Version { nums, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.nums.cmp(&other.nums).then_with(|| match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            // A pre-release comes before the release.
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => cmp_pre(a, b),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Pre-release identifiers, dot-separated: numbers compare numerically and before text.
fn cmp_pre(a: &str, b: &str) -> Ordering {
    let (mut ia, mut ib) = (a.split('.'), b.split('.'));
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

/// `true` if `candidate` is strictly newer than `current` (unparsable → `false`).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((Version::parse(candidate), Version::parse(current)), (Some(c), Some(cur)) if c > cur)
}

/// `releases/latest` response → the release, `Ok(None)` for a draft or a pre-release,
/// `Err` for unexpected JSON.
pub fn parse_latest(json: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let flag = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return Ok(None);
    }
    let field = |k: &str| v.get(k).and_then(|x| x.as_str()).ok_or_else(|| format!("missing `{k}`"));
    let tag = field("tag_name")?;
    let url = field("html_url")?;
    let version = Version::parse(tag).ok_or_else(|| format!("bad tag `{tag}`"))?;
    let [a, b, c] = version.nums;
    let version = match &version.pre {
        Some(p) => format!("{a}.{b}.{c}-{p}"),
        None => format!("{a}.{b}.{c}"),
    };
    Ok(Some(Release { version, url: url.to_string() }))
}

/// Check now? The setting is on and the last check is more than a day old (or never happened).
pub fn should_check(enabled: bool, last_check: Option<u64>, now: u64) -> bool {
    enabled && last_check.is_none_or(|t| now.saturating_sub(t) >= DAY || t > now)
}

/// Show the notice? Newer than the running version and not the skipped one.
pub fn should_notify(release: &str, current: &str, skipped: Option<&str>) -> bool {
    is_newer(release, current) && skipped.is_none_or(|s| Version::parse(s) != Version::parse(release))
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Starts the request in the background. The channel receives `Ok` once GitHub answered
/// (with or without a usable release), `Err` on any failure.
pub fn spawn_check() -> Receiver<Result<Option<Release>, String>> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
        let _ = tx.send(fetch_latest());
    });
    if let Err(e) = spawned {
        debug_log(&e.to_string());
    }
    rx
}

fn fetch_latest() -> Result<Option<Release>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
    let mut resp = agent
        .get(LATEST_URL)
        .header("User-Agent", &format!("sequensss/{CURRENT}"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    parse_latest(&body)
}

/// Failures are only visible in debug builds.
pub fn debug_log(msg: &str) {
    if cfg!(debug_assertions) {
        eprintln!("update check: {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("0.2.0", "0.1.99"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.3"));
    }

    #[test]
    fn v_prefix_is_ignored() {
        assert!(is_newer("v0.1.4", "0.1.3"));
        assert!(!is_newer("v0.1.3", "0.1.3"));
        assert_eq!(Version::parse("v1.2.3"), Version::parse("1.2.3"));
    }

    #[test]
    fn pre_releases_come_before_the_release() {
        assert!(is_newer("1.0.0", "1.0.0-beta"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
        assert!(is_newer("1.0.0-beta", "0.9.0"));
        assert!(is_newer("1.0.0-beta.11", "1.0.0-beta.2"));
        assert!(is_newer("1.0.0-beta", "1.0.0-alpha"));
        assert!(is_newer("1.0.0-alpha.beta", "1.0.0-alpha.1"));
    }

    #[test]
    fn bad_versions_are_rejected() {
        for s in ["", "v", "1.2", "1.2.3.4", "a.b.c", "1.2.3-", "latest"] {
            assert_eq!(Version::parse(s), None, "{s}");
        }
        assert!(!is_newer("garbage", "0.1.3"));
    }

    const SAMPLE: &str = r#"{
        "url": "https://api.github.com/repos/gurzil-io/sequensss/releases/1",
        "html_url": "https://github.com/gurzil-io/sequensss/releases/tag/v0.1.4",
        "id": 1,
        "tag_name": "v0.1.4",
        "name": "Sequensss 0.1.4",
        "draft": false,
        "prerelease": false,
        "assets": [{ "name": "sequensss-0.1.4.apk" }]
    }"#;

    #[test]
    fn parses_latest_release() {
        let r = parse_latest(SAMPLE).unwrap().unwrap();
        assert_eq!(r.version, "0.1.4");
        assert_eq!(r.url, "https://github.com/gurzil-io/sequensss/releases/tag/v0.1.4");
    }

    #[test]
    fn drafts_and_prereleases_are_skipped() {
        assert_eq!(parse_latest(&SAMPLE.replace(r#""draft": false"#, r#""draft": true"#)), Ok(None));
        assert_eq!(parse_latest(&SAMPLE.replace(r#""prerelease": false"#, r#""prerelease": true"#)), Ok(None));
    }

    #[test]
    fn unexpected_json_is_an_error() {
        assert!(parse_latest("not json").is_err());
        assert!(parse_latest(r#"{"message": "Not Found"}"#).is_err());
        assert!(parse_latest(r#"{"tag_name": "v0.1.4"}"#).is_err());
        assert!(parse_latest(r#"{"html_url": "https://x"}"#).is_err());
        assert!(parse_latest(r#"{"tag_name": "nightly", "html_url": "https://x"}"#).is_err());
        assert!(parse_latest(r#"{"tag_name": 4, "html_url": "https://x"}"#).is_err());
    }

    #[test]
    fn checks_at_most_once_a_day() {
        let now = 1_700_000_000;
        assert!(should_check(true, None, now));
        assert!(!should_check(false, None, now));
        assert!(!should_check(true, Some(now - 60), now));
        assert!(!should_check(true, Some(now - DAY + 1), now));
        assert!(should_check(true, Some(now - DAY), now));
        assert!(!should_check(false, Some(now - 2 * DAY), now));
        // Clock moved back: check again rather than wait.
        assert!(should_check(true, Some(now + DAY), now));
    }

    #[test]
    fn skipped_version_is_not_notified() {
        assert!(should_notify("0.1.4", "0.1.3", None));
        assert!(!should_notify("0.1.4", "0.1.3", Some("0.1.4")));
        assert!(!should_notify("0.1.4", "0.1.3", Some("v0.1.4")));
        // A newer release than the skipped one is shown again.
        assert!(should_notify("0.1.5", "0.1.3", Some("0.1.4")));
        assert!(!should_notify("0.1.3", "0.1.3", None));
        assert!(!should_notify("0.1.2", "0.1.3", None));
    }
}
