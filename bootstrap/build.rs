#[path = "prompt_parser.rs"]
mod prompt_parser;

// The build's version string: the release number for a release, else the last release,
// `+`, the commits since it, and the commit. Mirrors docs/frontends/distribution.md#versions.
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

// Emit the version, the last release, whether this build is that release, the
// commits after it (empty when unknown), and the commit (empty when unknown).
fn emit(version: &str, release: &str, after: &str, commit: &str) {
    println!("cargo:rustc-env=JAZYK_VERSION={version}");
    println!("cargo:rustc-env=JAZYK_RELEASE={release}");
    println!("cargo:rustc-env=JAZYK_COMMITS_AFTER={after}");
    println!("cargo:rustc-env=JAZYK_COMMIT={commit}");
}

fn prepare_payloads(root: &Path, directory: &Path, out: &Path) {
    println!("cargo:rerun-if-changed={}", directory.display());
    for entry in std::fs::read_dir(directory).expect("payload directory") {
        let path = entry.expect("payload entry").path();
        if path.is_dir() {
            prepare_payloads(root, &path, out);
            continue;
        }
        let name = path.to_string_lossy();
        if !(name.ends_with(".md") || name.ends_with(".md.mustache")) {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let prepared =
            prompt_parser::load(root, &path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let relative = path.strip_prefix(root).expect("payload root");
        let dest = out.join(relative);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, prepared).unwrap();
    }
}

fn main() {
    println!("cargo:rerun-if-changed=prompt_parser.rs");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = if manifest.join("docs/compiler").is_dir() {
        manifest.join("docs/compiler")
    } else {
        manifest.join("../docs/compiler")
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payloads");
    prepare_payloads(&root, &root.join("goals/prompts"), &out);
    prepare_payloads(&root, &root.join("skills"), &out);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=JAZYK_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=JAZYK_BUILD_COMMIT");
    let package = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());

    // 1. The builder says: the release workflow and Jazyk Cloud know better than a
    //    checkout without tags or history.
    if let Ok(given) = std::env::var("JAZYK_BUILD_VERSION") {
        let given = given.trim().trim_start_matches('v').to_string();
        if !given.is_empty() {
            let commit = std::env::var("JAZYK_BUILD_COMMIT").unwrap_or_default();
            let (release, rest) = given.split_once('+').unwrap_or((&given, ""));
            let after = rest
                .split('.')
                .next()
                .filter(|n| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty())
                .unwrap_or(if rest.is_empty() { "0" } else { "" })
                .to_string();
            emit(&given, release, &after, commit.trim());
            return;
        }
    }

    // 2. A packaged crate (`cargo install jazyk`) is a published release.
    if manifest.join(".cargo_vcs_info.json").exists() {
        emit(&package, &package, "0", "");
        return;
    }

    // 3. The Jazyk checkout the source sits in. A checkout that is not this
    //    repository (the crate copied into another repo, or a home directory under
    //    Git) is no evidence.
    let repo = manifest.parent().map(Path::to_path_buf).unwrap_or_default();
    let top = git(&repo, &["rev-parse", "--show-toplevel"]).map(PathBuf::from);
    let same = top
        .as_deref()
        .and_then(|t| t.canonicalize().ok())
        .zip(repo.canonicalize().ok())
        .is_some_and(|(t, r)| t == r);
    if same {
        // Rebuild when the commit or the tags move.
        for p in ["HEAD", "packed-refs", "refs/tags"] {
            if let Some(path) = git(&repo, &["rev-parse", "--git-path", p]) {
                println!("cargo:rerun-if-changed={}", repo.join(path).display());
            }
        }
        if let Some(head_ref) = git(&repo, &["symbolic-ref", "-q", "HEAD"]) {
            if let Some(path) = git(&repo, &["rev-parse", "--git-path", &head_ref]) {
                println!("cargo:rerun-if-changed={}", repo.join(path).display());
            }
        }
        let commit = git(&repo, &["rev-parse", "HEAD"]).unwrap_or_default();
        let short = git(&repo, &["rev-parse", "--short=7", "HEAD"]).unwrap_or_default();
        let described = git(
            &repo,
            &[
                "describe",
                "--tags",
                "--long",
                "--abbrev=7",
                "--match",
                "v[0-9]*",
            ],
        );
        // `v0.2.2-31-ge0fff82`: the release, the commits after it, the commit.
        if let Some((tag, after)) = described.as_deref().and_then(|d| {
            let mut parts = d.rsplitn(3, '-');
            let _hash = parts.next()?;
            let after = parts.next()?;
            let tag = parts.next()?;
            Some((tag.trim_start_matches('v').to_string(), after.to_string()))
        }) {
            if after == "0" {
                emit(&tag, &tag, "0", &commit);
            } else {
                emit(&format!("{tag}+{after}.g{short}"), &tag, &after, &commit);
            }
            return;
        }
        if !short.is_empty() {
            // No release tag reachable: a shallow or tagless checkout.
            emit(&format!("{package}+g{short}"), &package, "", &commit);
            return;
        }
    }

    // 4. No evidence: an unreleased build of unknown origin.
    emit(&format!("{package}+unknown"), &package, "", "");
}
