//! HTTPS updates with validated staging, immutable packs and atomic publication.
use crate::{
    APP,
    bundle::ScriptBundle,
    data::{self, CATEGORIES},
    resources,
    storage::{self, atomic_write},
};
use reqwest::{Url, blocking::Client, header::ETAG};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type UpdateResult<T> = Result<T, String>;
const CHALDEA: &str = "https://raw.githubusercontent.com/chaldea-center/chaldea-data/main/mappings";
const MAX_JSON: u64 = 16 * 1024 * 1024;
const MAX_PACK: u64 = 512 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub auto_update: bool,
    pub update_interval_hours: u64,
    /// Optional HTTPS manifest for stories, UI, localization and the font bundle.
    pub resource_manifest_url: Option<String>,
    pub record_missing: bool,
    pub export_login_response: bool,
    pub check_apk_updates: bool,
    pub download_apk_updates: bool,
    /// HTTPS prefix for public GitHub resources. Empty/null uses GitHub directly.
    pub github_mirror: Option<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            auto_update: true,
            update_interval_hours: 24,
            resource_manifest_url: Some(
                "https://github.com/jhq223/cleat-fgo/releases/download/resources/manifest.json"
                    .into(),
            ),
            record_missing: true,
            export_login_response: true,
            check_apk_updates: true,
            download_apk_updates: true,
            github_mirror: Some("https://gh-proxy.com".into()),
        }
    }
}
impl Config {
    pub fn load(base: &Path) -> Self {
        let path = base.join("config.json");
        if !path.exists() {
            let config = Self::default();
            if let Ok(bytes) = serde_json::to_vec_pretty(&config) {
                let _ = atomic_write(&path, &bytes);
            }
            return config;
        }
        match fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
        {
            Ok(config) => config,
            Err(e) => {
                log::warn!("config: {e}; using defaults");
                Self::default()
            }
        }
    }
}
#[derive(Default, Deserialize, Serialize)]
struct State {
    #[serde(default)]
    apk_checked: u64,
    #[serde(default)]
    mappings_checked: u64,
    #[serde(default)]
    pack_checked: u64,
    #[serde(default)]
    manifest_url: Option<String>,
    #[serde(default)]
    etags: HashMap<String, String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    pub files: Vec<ResourceFile>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceFile {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

pub fn start() {
    let Some(app) = APP.get() else {
        return;
    };
    if !app.config.auto_update && !app.config.check_apk_updates {
        return;
    }
    std::thread::spawn(|| {
        let builder = Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("cleat-fgo/", env!("CARGO_PKG_VERSION")));
        let client = match builder.build() {
            Ok(client) => client,
            Err(e) => {
                log::warn!("updates: {e}");
                return;
            }
        };
        loop {
            let Some(app) = APP.get() else {
                return;
            };
            let state_path = app.base.join("cache/update-state.json");
            let mut state: State = fs::read(&state_path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            let now = timestamp();
            let interval = app.config.update_interval_hours.clamp(1, 168) * 3600;
            let mut changed = false;
            if app.config.auto_update
                && (now.saturating_sub(state.mappings_checked) >= interval
                    || !app.base.join("cache/mappings").is_dir())
            {
                match update_mappings(&client, &app.base, &mut state) {
                    Ok(updated) => {
                        changed |= updated;
                        state.mappings_checked = now;
                    }
                    Err(e) => {
                        log::warn!("Chaldea update: {e}; keeping available translations");
                        changed = true;
                    }
                }
            }
            if let Some(url) = app
                .config
                .resource_manifest_url
                .as_deref()
                .filter(|s| app.config.auto_update && !s.is_empty())
                && (state.manifest_url.as_deref() != Some(url)
                    || now.saturating_sub(state.pack_checked) >= interval)
            {
                match update_pack(&client, &app.base, url) {
                    Ok(updated) => {
                        changed |= updated;
                        state.pack_checked = now;
                        state.manifest_url = Some(url.into());
                    }
                    Err(e) => {
                        log::warn!("resource update: {e}; keeping the previous complete pack")
                    }
                }
            }
            if changed {
                app.reload();
            }
            #[cfg(target_os = "android")]
            if app.config.check_apk_updates && now.saturating_sub(state.apk_checked) >= interval {
                match crate::apk_update::check(&client, &app.base, app.config.download_apk_updates)
                {
                    Ok(()) => state.apk_checked = now,
                    Err(error) => log::warn!("APK update: {error}"),
                }
            }
            if let Ok(bytes) = serde_json::to_vec_pretty(&state) {
                let _ = atomic_write(&state_path, &bytes);
            }
            // Retry a failed connection later without blocking game startup.
            std::thread::sleep(Duration::from_secs(15 * 60));
        }
    });
}
fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn update_mappings(client: &Client, base: &Path, state: &mut State) -> UpdateResult<bool> {
    let dir = base.join("cache/mappings");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut changed = false;
    let mut failures = 0;
    let mut updated = 0;
    for category in CATEGORIES {
        let file = format!("{category}.json");
        let path = dir.join(&file);
        let result = (|| {
            let etag = path
                .is_file()
                .then(|| state.etags.get(&file))
                .flatten()
                .map(String::as_str);
            let response = crate::network::get(client, &format!("{CHALDEA}/{file}"), etag)?;
            if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(false);
            }
            let response = response.error_for_status().map_err(|e| e.to_string())?;
            let etag = response
                .headers()
                .get(ETAG)
                .and_then(|h| h.to_str().ok())
                .map(str::to_owned);
            let bytes = limited(response, MAX_JSON)?;
            data::parse_mapping(&bytes)?;
            let same = fs::read(&path).is_ok_and(|old| old == bytes);
            if !same {
                atomic_write(&path, &bytes)?;
            }
            if let Some(etag) = etag {
                state.etags.insert(file.clone(), etag);
            }
            Ok::<_, String>(!same)
        })();
        match result {
            Ok(value) => {
                changed |= value;
                updated += usize::from(value);
            }
            Err(e) => {
                failures += 1;
                log::warn!("Chaldea {file}: {e}");
                if failures == 3 {
                    break;
                }
            }
        }
    }
    log::info!("Chaldea: {updated} categories updated, {failures} failures");
    if failures > 0 {
        Err(format!("{failures} downloads failed"))
    } else {
        Ok(changed)
    }
}
fn validate_manifest(manifest: &Manifest) -> UpdateResult<()> {
    if manifest.schema != 1 || manifest.version.is_empty() || manifest.files.len() > 1000 {
        return Err("unsupported manifest schema, version or file count".into());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for file in &manifest.files {
        if !safe_name(&file.name) || !names.insert(&file.name) {
            return Err(format!("invalid or duplicate resource name: {}", file.name));
        }
        if file.size == 0 || file.size > crate::bundle::MAX_BUNDLE {
            return Err(format!("invalid resource size: {}", file.name));
        }
        total = total.checked_add(file.size).ok_or("pack size overflow")?;
        if total > MAX_PACK {
            return Err("resource pack exceeds size limit; split into smaller chapters".into());
        }
        if file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("invalid SHA-256: {}", file.name));
        }
        https_url(&file.url)?;
    }
    Ok(())
}
fn https_url(value: &str) -> UpdateResult<Url> {
    let url = Url::parse(value).map_err(|e| e.to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("resource URL must use HTTPS without credentials".into());
    }
    Ok(url)
}
fn safe_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 120
        || name.starts_with('.')
        || name.contains("..")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return false;
    }
    name == "Font"
        || name == "LocalizationJpn.txt"
        || name == "ui.json"
        || name.ends_with(".script")
        || CATEGORIES.iter().any(|c| name == format!("{c}.json"))
        || name == "event_mission.json"
}
fn update_pack(client: &Client, base: &Path, manifest_url: &str) -> UpdateResult<bool> {
    let bytes = crate::network::bytes(client, manifest_url, 1024 * 1024, Duration::from_secs(120))?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate_manifest(&manifest)?;
    let generation = hash(&bytes);
    let cache = base.join("cache");
    let active = cache.join("active-pack");
    let packs = cache.join("packs");
    let destination = packs.join(&generation);
    if fs::read_to_string(&active).is_ok_and(|old| old.trim() == generation) && destination.is_dir()
    {
        return Ok(false);
    }
    fs::create_dir_all(&packs).map_err(|e| e.to_string())?;
    let staging = tempfile::tempdir_in(&packs).map_err(|e| e.to_string())?;
    storage::allow_group_access(staging.path()).map_err(|e| e.to_string())?;
    let blobs = cache.join("blobs");
    fs::create_dir_all(&blobs).map_err(|e| e.to_string())?;
    for file in &manifest.files {
        let digest = file.sha256.to_ascii_lowercase();
        let blob = blobs.join(&digest);
        let bytes = match fs::read(&blob) {
            Ok(bytes) if verify_blob(file, &bytes).is_ok() => bytes,
            _ => {
                log::info!("downloading resource: {} ({} bytes)", file.name, file.size);
                crate::network::download(client, &file.url, &blob, file.size, &digest)
                    .map_err(|error| format!("{}: {error}", file.name))?;
                let bytes = fs::read(&blob).map_err(|e| e.to_string())?;
                verify_blob(file, &bytes)?;
                validate_resource(&file.name, &bytes)?;
                bytes
            }
        };
        validate_resource(&file.name, &bytes)?;
        atomic_write(&staging.path().join(&file.name), &bytes)?;
    }
    if !destination.exists() {
        fs::rename(staging.path(), &destination).map_err(|e| e.to_string())?;
    }
    atomic_write(&active, generation.as_bytes())?;
    log::info!(
        "resource pack {} published ({} files)",
        manifest.version,
        manifest.files.len()
    );
    Ok(true)
}
fn verify_blob(file: &ResourceFile, bytes: &[u8]) -> UpdateResult<()> {
    if bytes.len() as u64 != file.size || !hash(bytes).eq_ignore_ascii_case(&file.sha256) {
        return Err(format!("size/hash mismatch: {}", file.name));
    }
    Ok(())
}
fn validate_resource(name: &str, bytes: &[u8]) -> UpdateResult<()> {
    if name.ends_with(".script") {
        ScriptBundle::parse(bytes).map(|_| ())
    } else if name == "ui.json" {
        data::parse_ui(bytes).map(|_| ())
    } else if name == "LocalizationJpn.txt" {
        resources::localization_text(bytes).map(|_| ())
    } else if name == "Font" {
        if bytes.starts_with(b"UnityFS\0") || bytes.starts_with(b"UnityRaw\0") {
            Ok(())
        } else {
            Err("Font must be a Unity AssetBundle".into())
        }
    } else {
        data::parse_mapping(bytes).map(|_| ())
    }
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub(crate) fn limited(
    mut response: reqwest::blocking::Response,
    max: u64,
) -> UpdateResult<Vec<u8>> {
    if response.content_length().is_some_and(|length| length > max) {
        return Err("download exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    let mut buffer = [0; 64 * 1024];
    let mut reported = 0;
    loop {
        let count = response
            .read(&mut buffer)
            .map_err(|e| format!("received {} bytes: {e:?}", bytes.len()))?;
        if count == 0 {
            break;
        }
        if bytes.len() as u64 + count as u64 > max {
            return Err("download exceeds size limit".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() / (4 * 1024 * 1024) > reported {
            reported = bytes.len() / (4 * 1024 * 1024);
            log::info!("download progress: {} bytes", bytes.len());
        }
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_traversal_bad_hash_and_non_https() {
        for name in [
            "../ui.json",
            "/ui.json",
            "C:ui.json",
            "a/b.script",
            "..script",
            "module.so",
        ] {
            assert!(!safe_name(name));
        }
        assert!(safe_name("chapter-1.script"));
        assert!(https_url("http://example.org/x").is_err());
        let file = ResourceFile {
            name: "ui.json".into(),
            url: "https://example.org/ui.json".into(),
            size: 2,
            sha256: hash(b"{}"),
        };
        assert!(verify_blob(&file, b"{}").is_ok());
        assert!(verify_blob(&file, b"[]").is_err());
        assert!(
            validate_manifest(&Manifest {
                schema: 1,
                version: "1".into(),
                files: vec![file]
            })
            .is_ok()
        );
    }
}
