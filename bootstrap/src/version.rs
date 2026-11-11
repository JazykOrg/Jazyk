// The build's version, computed by build.rs. Mirrors docs/frontends/distribution.md#versions.

// The version string: `0.2.2` for a release, `0.2.2+31.ge0fff82` for a build 31
// commits after it, `0.2.2+ge0fff82` when the commits cannot be counted, and
// `0.2.2+unknown` with no evidence at all.
pub const VERSION: &str = env!("JAZYK_VERSION");
// The last release at or before this build.
pub const RELEASE: &str = env!("JAZYK_RELEASE");
// Commits after `RELEASE`: `0` for the release itself, empty when unknown.
pub const COMMITS_AFTER: &str = env!("JAZYK_COMMITS_AFTER");
// The full commit hash, empty when unknown.
pub const COMMIT: &str = env!("JAZYK_COMMIT");

// Whether this build is a release.
pub fn released() -> bool {
    VERSION == RELEASE
}

// The version line: `jazyk 0.2.2`, or what an unreleased build is relative to the
// last release.
pub fn line() -> String {
    if released() {
        return format!("jazyk {}", VERSION);
    }
    let what = match COMMITS_AFTER {
        "" => format!("unreleased, after {}", RELEASE),
        "1" => format!("unreleased, 1 commit after {}", RELEASE),
        n => format!("unreleased, {} commits after {}", n, RELEASE),
    };
    format!("jazyk {} ({})", VERSION, what)
}

// Everything `jazyk version --json` prints and the APIs report.
pub fn json() -> serde_json::Value {
    serde_json::json!({
        "version": VERSION,
        "release": RELEASE,
        "released": released(),
        "commitsAfter": COMMITS_AFTER.parse::<u64>().ok(),
        "commit": COMMIT,
        "store": crate::store::STORE_VERSION,
        "protocol": crate::protocol::EDITOR_PROTOCOL_VERSION,
    })
}

#[cfg(test)]
mod tests {
    // The version names a release or says how it relates to one.
    #[test]
    fn the_version_names_its_release() {
        let v = super::VERSION;
        assert!(v.starts_with(super::RELEASE), "{v}");
        if super::released() {
            assert_eq!(super::line(), format!("jazyk {}", v));
        } else {
            assert!(v.contains('+'), "{v}");
            assert!(super::line().contains("unreleased"), "{}", super::line());
        }
        assert_eq!(super::json()["store"], crate::store::STORE_VERSION);
    }
}
