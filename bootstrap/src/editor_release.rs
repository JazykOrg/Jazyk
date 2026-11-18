//! Thin editor adapters are versioned GitHub assets, independent of the native runtime.
use sha2::{Digest, Sha256};
use std::{io::Read, path::PathBuf, time::Duration};
pub const VERSION: &str = "0.2.4";
fn base_url() -> String {
    format!("https://github.com/JazykOrg/Jazyk/releases/download/editors-v{VERSION}")
}
fn cache() -> Result<PathBuf, String> {
    dirs::cache_dir()
        .map(|p| p.join("jazyk/editors").join(VERSION))
        .ok_or("cannot locate editor download cache".into())
}
fn checksum(text: &str, name: &str) -> Result<String, String> {
    text.lines()
        .find_map(|l| {
            let mut parts = l.split_whitespace();
            let hash = parts.next()?;
            let file = parts.next()?.trim_start_matches('*');
            (file == name && hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
                .then(|| hash.to_ascii_lowercase())
        })
        .ok_or_else(|| format!("editor release has no valid SHA-256 for {name}"))
}
fn download(name: &str) -> Result<Vec<u8>, String> {
    let url = format!("{}/{name}", base_url());
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let response=agent.get(&url).call().map_err(|e|format!("cannot download {url}: {e}; publish editor release editors-v{VERSION} or build a local adapter"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("editor download exceeds 64 MiB".into());
    }
    Ok(bytes)
}
pub fn asset(name: &str) -> Result<PathBuf, String> {
    if !matches!(
        name,
        "jazyk.vsix" | "jazyk-jetbrains.zip" | "jazyk-zed.zip" | "updatePlugins.xml"
    ) {
        return Err("unknown editor artifact".into());
    }
    let dir = std::env::var_os("JAZYK_EDITOR_ARTIFACT_DIR")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(cache)?;
    let sums = dir.join("SHA256SUMS");
    let checks = match std::fs::read_to_string(&sums) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let bytes = download("SHA256SUMS")?;
            let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            crate::editor_lsp::write_atomic(&sums, text.as_bytes())?;
            text
        }
        Err(e) => return Err(format!("{}: {e}", sums.display())),
    };
    let expected = checksum(&checks, name)?;
    let path = dir.join(name);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => download(name)?,
        Err(e) => return Err(e.to_string()),
    };
    if format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err(format!(
            "SHA-256 mismatch for {}; remove the damaged cached artifact and retry",
            path.display()
        ));
    }
    crate::editor_lsp::write_atomic(&path, &bytes)?;
    Ok(path)
}
pub fn jetbrains_repository() -> Result<String, String> {
    // Resolve and verify both before invoking the native installer.
    asset("jazyk-jetbrains.zip")?;
    asset("updatePlugins.xml")?;
    if std::env::var_os("JAZYK_EDITOR_ARTIFACT_DIR").is_some() {
        let path = asset("updatePlugins.xml")?;
        return reqwest::Url::from_file_path(path)
            .map(|url| url.to_string())
            .map_err(|_| "invalid local plugin repository path".into());
    }
    Ok(format!("{}/updatePlugins.xml", base_url()))
}
pub fn zed_source() -> Result<PathBuf, String> {
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("editors/zed");
    if local.join("Cargo.toml").is_file() {
        return Ok(local);
    }
    let archive = asset("jazyk-zed.zip")?;
    #[cfg(feature = "native-documents")]
    {
        let dir = archive.parent().unwrap().join("zed");
        let bytes = std::fs::read(&archive).map_err(|e| e.to_string())?;
        let mut zip =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let relative = entry
                .enclosed_name()
                .ok_or("unsafe path in editor archive")?;
            if entry.is_dir() {
                continue;
            }
            if entry.is_symlink() || entry.size() > 8 * 1024 * 1024 {
                return Err("invalid Zed adapter source entry".into());
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            crate::editor_lsp::write_atomic(&dir.join(relative), &bytes)?;
        }
        Ok(dir)
    }
    #[cfg(not(feature = "native-documents"))]
    Err(format!(
        "extract {} and use Zed's Install Dev Extension action",
        archive.display()
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checksum_requires_the_exact_asset_and_full_digest() {
        let text = format!(
            "{}  jazyk.vsix\n{} *jazyk-jetbrains.zip\n",
            "a".repeat(64),
            "B".repeat(64)
        );
        assert_eq!(checksum(&text, "jazyk.vsix").unwrap(), "a".repeat(64));
        assert_eq!(
            checksum(&text, "jazyk-jetbrains.zip").unwrap(),
            "b".repeat(64)
        );
        assert!(checksum(&text, "other.vsix").is_err());
        assert!(checksum("bad  jazyk.vsix", "jazyk.vsix").is_err());
    }
}
