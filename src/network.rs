//! Public resource downloads with mirror fallback.
use reqwest::{
    Url,
    blocking::{Client, Response},
    header::IF_NONE_MATCH,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    time::Duration,
};

pub fn github_url(original: &str, mirror: Option<&str>) -> Result<String, String> {
    let url = Url::parse(original).map_err(|e| e.to_string())?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err("download URL must use HTTPS without credentials".into());
    }
    let Some(mirror) = mirror.filter(|m| !m.is_empty()) else {
        return Ok(original.into());
    };
    if !matches!(
        url.host_str(),
        Some(
            "github.com"
                | "api.github.com"
                | "raw.githubusercontent.com"
                | "codeload.github.com"
                | "gist.githubusercontent.com"
        )
    ) {
        return Ok(original.into());
    }
    let base = Url::parse(mirror).map_err(|_| "invalid GitHub mirror URL")?;
    if base.scheme() != "https"
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(
            "GitHub mirror must be an HTTPS prefix without credentials or query parameters".into(),
        );
    }
    Ok(format!("{}/{}", mirror.trim_end_matches('/'), original))
}

pub fn get(client: &Client, original: &str, etag: Option<&str>) -> Result<Response, String> {
    get_with_timeout(client, original, etag, std::time::Duration::from_secs(120))
}

pub fn get_with_timeout(
    client: &Client,
    original: &str,
    etag: Option<&str>,
    timeout: std::time::Duration,
) -> Result<Response, String> {
    with_response(client, original, etag, timeout, Ok)
}

pub fn with_response<T>(
    client: &Client,
    original: &str,
    etag: Option<&str>,
    timeout: std::time::Duration,
    mut consume: impl FnMut(Response) -> Result<T, String>,
) -> Result<T, String> {
    let mirror = crate::APP
        .get()
        .and_then(|app| app.config.github_mirror.as_deref());
    let routed = github_url(original, mirror)?;
    try_urls(&routed, original, |url| {
        let mut request = client.get(url).timeout(timeout);
        if let Some(etag) = etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        request
            .send()
            .and_then(Response::error_for_status)
            .map_err(|e| format!("{e:?}"))
            .and_then(&mut consume)
    })
}

fn try_urls<T>(
    primary: &str,
    original: &str,
    mut request: impl FnMut(&str) -> Result<T, String>,
) -> Result<T, String> {
    match request(primary) {
        Err(error) if primary != original => {
            log::warn!("GitHub mirror failed; retrying the original URL: {error}");
            request(original)
        }
        result => result,
    }
}

pub fn bytes(
    client: &Client,
    url: &str,
    max: u64,
    timeout: std::time::Duration,
) -> Result<Vec<u8>, String> {
    with_response(client, url, None, timeout, |response| {
        crate::update::limited(response, max)
    })
}

/// Download in resumable ranges. The final path appears only after SHA-256 validation.
pub fn download(
    client: &Client,
    url: &str,
    path: &Path,
    size: u64,
    sha256: &str,
) -> Result<(), String> {
    let mirror = crate::APP
        .get()
        .and_then(|app| app.config.github_mirror.as_deref());
    let routed = github_url(url, mirror)?;
    download_from(client, &routed, url, path, size, sha256)
}

fn download_from(
    client: &Client,
    primary: &str,
    original: &str,
    path: &Path,
    size: u64,
    sha256: &str,
) -> Result<(), String> {
    if file_matches(path, size, sha256)? {
        return Ok(());
    }
    fs::create_dir_all(path.parent().ok_or("missing download directory")?)
        .map_err(|e| e.to_string())?;
    let partial = path.with_extension("part");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&partial)
        .map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > size {
        file.set_len(0).map_err(|e| e.to_string())?;
    }
    let mut failures = 0;
    while file.metadata().map_err(|e| e.to_string())?.len() < size {
        let result = try_urls(primary, original, |url| {
            let offset = file.metadata().map_err(|e| e.to_string())?.len();
            if offset == size {
                return Ok(());
            }
            let end = (offset + 1024 * 1024 - 1).min(size - 1);
            let response = client
                .get(url)
                .header(reqwest::header::RANGE, format!("bytes={offset}-{end}"))
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .timeout(Duration::from_secs(90))
                .send()
                .and_then(Response::error_for_status)
                .map_err(|e| e.to_string())?;
            let expected = if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                let expected_range = format!("bytes {offset}-{end}/{size}");
                if response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|h| h.to_str().ok())
                    != Some(expected_range.as_str())
                {
                    return Err("download returned an unexpected byte range".into());
                }
                end - offset + 1
            } else if response.status() == reqwest::StatusCode::OK {
                // Servers without Range support send the complete representation.
                size
            } else {
                return Err(format!("unexpected download status: {}", response.status()));
            };
            if response
                .content_length()
                .is_some_and(|length| length != expected)
            {
                return Err("download response length mismatch".into());
            }
            if response.status() == reqwest::StatusCode::OK {
                file.set_len(0)
                    .and_then(|()| file.rewind())
                    .map_err(|e| e.to_string())?;
            } else {
                file.seek(SeekFrom::Start(offset))
                    .map_err(|e| e.to_string())?;
            }
            let copied = std::io::copy(&mut response.take(expected), &mut file)
                .map_err(|e| e.to_string())?;
            file.flush()
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            if copied != expected {
                return Err("download response was truncated".into());
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                failures = 0;
                log::info!(
                    "download {}: {}/{} bytes",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    file.metadata().map_err(|e| e.to_string())?.len(),
                    size
                );
            }
            Err(error) => {
                failures += 1;
                if failures >= 3 {
                    return Err(error);
                }
                log::warn!("download interrupted; resuming: {error}");
            }
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    if !file_matches(&partial, size, sha256)? {
        fs::remove_file(&partial).map_err(|e| e.to_string())?;
        return Err("download SHA-256 mismatch".into());
    }
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    fs::rename(partial, path).map_err(|e| e.to_string())
}

pub fn file_matches(path: &Path, size: u64, sha256: &str) -> Result<bool, String> {
    let Ok(mut file) = fs::File::open(path) else {
        return Ok(false);
    };
    if file.metadata().map_err(|e| e.to_string())?.len() != size {
        return Ok(false);
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()).eq_ignore_ascii_case(sha256))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resumes_a_truncated_mirror_at_the_origin_and_verifies_the_result() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for (range, response) in [
                (
                    "range: bytes=0-5",
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-5/6\r\nContent-Length: 6\r\nConnection: close\r\n\r\nab",
                ),
                (
                    "range: bytes=2-5",
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-5/6\r\nContent-Length: 4\r\nConnection: close\r\n\r\ncdef",
                ),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 2048];
                let count = socket.read(&mut request).unwrap();
                assert!(
                    String::from_utf8_lossy(&request[..count])
                        .to_ascii_lowercase()
                        .contains(range)
                );
                socket.write_all(response.as_bytes()).unwrap();
            }
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("download");
        let client = Client::builder().no_proxy().build().unwrap();
        download_from(
            &client,
            &format!("http://{address}/mirror"),
            &format!("http://{address}/origin"),
            &path,
            6,
            &hex::encode(Sha256::digest(b"abcdef")),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"abcdef");
        assert!(!path.with_extension("part").exists());
        server.join().unwrap();
    }

    #[test]
    fn completed_partial_download_is_verified_before_publication() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("download");
        let partial = path.with_extension("part");
        let digest = hex::encode(Sha256::digest(b"good"));
        let client = Client::new();
        fs::write(&partial, b"oops").unwrap();
        assert!(download_from(&client, "", "", &path, 4, &digest).is_err());
        assert!(!path.exists());
        assert!(!partial.exists());
        fs::write(&partial, b"good").unwrap();
        download_from(&client, "", "", &path, 4, &digest).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"good");
    }
    #[test]
    fn mirrors_only_explicit_github_hosts() {
        let prefix = Some("https://gh-proxy.com/");
        assert_eq!(
            github_url("https://raw.githubusercontent.com/a/b/main/x.json", prefix).unwrap(),
            "https://gh-proxy.com/https://raw.githubusercontent.com/a/b/main/x.json"
        );
        assert_eq!(
            github_url("https://game.fate-go.jp/top", prefix).unwrap(),
            "https://game.fate-go.jp/top"
        );
        assert_eq!(
            github_url("https://github.com.example.com/file", prefix).unwrap(),
            "https://github.com.example.com/file"
        );
        assert!(github_url("https://github.com/a", Some("http://mirror.example")).is_err());
    }

    #[test]
    fn retries_the_origin_after_a_truncated_mirror_body() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for body in [
                b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\nConnection: close\r\n\r\ncut".as_slice(),
                b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nok!".as_slice(),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut buffer = [0; 2048];
                let _ = socket.read(&mut buffer).unwrap();
                socket.write_all(body).unwrap();
            }
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let primary = format!("http://{address}/mirror");
        let origin = format!("http://{address}/origin");
        let bytes = try_urls(&primary, &origin, |url| {
            let response = client.get(url).send().unwrap();
            crate::update::limited(response, 100)
        })
        .unwrap();
        assert_eq!(bytes, b"ok!");
        server.join().unwrap();
    }
}
