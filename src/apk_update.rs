//! GitHub APK releases: compare installed versions, stream and verify downloads.
#[cfg(target_os = "android")]
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs;
use std::path::Path;

#[cfg(target_os = "android")]
const RELEASE_API: &str = "https://api.github.com/repos/jhq223/cleat-fgo-build/releases/latest";
const MAX_APK: u64 = 1024 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
pub struct ApkManifest {
    schema: u32,
    version: String,
    version_code: i64,
    module_version: String,
    url: String,
    size: u64,
    sha256: String,
}

impl ApkManifest {
    fn validate(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.url).map_err(|e| e.to_string())?;
        if self.schema != 1
            || self.version.is_empty()
            || self.version_code <= 0
            || self.size == 0
            || self.size > MAX_APK
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url
                .path()
                .starts_with("/jhq223/cleat-fgo-build/releases/download/")
            || !url.path().ends_with("/fgo-mod.apk")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("invalid APK release manifest".into());
        }
        semver::Version::parse(self.module_version.trim_start_matches('v'))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn newer_than(&self, installed_code: i64, module: &str) -> Result<bool, String> {
        let latest = semver::Version::parse(self.module_version.trim_start_matches('v'))
            .map_err(|e| e.to_string())?;
        let installed = semver::Version::parse(module).map_err(|e| e.to_string())?;
        Ok(self.version_code > installed_code
            || (self.version_code == installed_code && latest > installed))
    }
}

#[cfg(target_os = "android")]
pub fn check(client: &Client, base: &Path, download: bool) -> Result<(), String> {
    let bytes = crate::network::bytes(
        client,
        RELEASE_API,
        1024 * 1024,
        std::time::Duration::from_secs(120),
    )?;
    let release: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let Some(url) = manifest_url(&release) else {
        log::info!("APK release has no update manifest; keeping the installed version");
        return Ok(());
    };
    let bytes = crate::network::bytes(client, url, 64 * 1024, std::time::Duration::from_secs(120))?;
    let manifest: ApkManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    manifest.validate()?;
    if !manifest.newer_than(crate::android::version_code()?, env!("CARGO_PKG_VERSION"))? {
        return Ok(());
    }
    crate::storage::atomic_write(
        &base.join("updates/available.json"),
        &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )?;
    log::info!(
        "APK update available: {} (module {})",
        manifest.version,
        manifest.module_version
    );
    if !download {
        return Ok(());
    }
    let path = base
        .join("updates")
        .join(format!("fgo-{}.apk", manifest.sha256.to_ascii_lowercase()));
    let marker = path.with_extension("notified");
    if !file_matches(&path, &manifest)? {
        download_apk(client, &path, &manifest)?;
    }
    crate::android::validate_apk(&path, manifest.version_code)?;
    if !marker.exists() {
        crate::android::notify_apk(&path, manifest.size)?;
        crate::storage::atomic_write(&marker, b"1")?;
    }
    log::info!("verified APK ready: {}", path.display());
    Ok(())
}

fn manifest_url(release: &serde_json::Value) -> Option<&str> {
    if release.get("draft")?.as_bool()? || release.get("prerelease")?.as_bool()? {
        return None;
    }
    release.get("assets")?.as_array()?.iter().find_map(|a| {
        if a.get("name")?.as_str()? != "update.json" {
            return None;
        }
        let url = a.get("browser_download_url")?.as_str()?;
        let parsed = reqwest::Url::parse(url).ok()?;
        (parsed.scheme() == "https"
            && parsed.host_str() == Some("github.com")
            && parsed
                .path()
                .starts_with("/jhq223/cleat-fgo-build/releases/download/")
            && parsed.path().ends_with("/update.json"))
        .then_some(url)
    })
}

fn file_matches(path: &Path, manifest: &ApkManifest) -> Result<bool, String> {
    crate::network::file_matches(path, manifest.size, &manifest.sha256)
}

#[cfg(target_os = "android")]
fn download_apk(client: &Client, path: &Path, manifest: &ApkManifest) -> Result<(), String> {
    crate::network::download(client, &manifest.url, path, manifest.size, &manifest.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> ApkManifest {
        ApkManifest {
            schema: 1,
            version: "v2.140.0".into(),
            version_code: 503,
            module_version: "0.2.0".into(),
            url: "https://github.com/jhq223/cleat-fgo-build/releases/download/v2.140.0/fgo-mod.apk"
                .into(),
            size: 3,
            sha256: hex::encode(Sha256::digest(b"apk")),
        }
    }
    #[test]
    fn never_downgrades_and_checks_same_game_module_updates() {
        let m = manifest();
        m.validate().unwrap();
        assert!(m.newer_than(502, "0.2.0").unwrap());
        assert!(m.newer_than(503, "0.1.3").unwrap());
        assert!(!m.newer_than(503, "0.2.0").unwrap());
        assert!(!m.newer_than(504, "0.1.0").unwrap());
        let mut m = m;
        m.url = "https://github.com.example.org/fgo-mod.apk".into();
        assert!(m.validate().is_err());
    }
    #[test]
    fn rejects_corrupt_cached_apks_and_non_release_assets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.apk");
        fs::write(&path, b"apk").unwrap();
        assert!(file_matches(&path, &manifest()).unwrap());
        fs::write(&path, b"bad").unwrap();
        assert!(!file_matches(&path, &manifest()).unwrap());
        assert!(
            manifest_url(&serde_json::json!({"draft":true,"prerelease":false,"assets":[]}))
                .is_none()
        );
    }
}
