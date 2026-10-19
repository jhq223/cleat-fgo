//! Public resource downloads with mirror fallback.
use reqwest::{
    Url,
    blocking::{Client, Response},
    header::IF_NONE_MATCH,
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

#[cfg(test)]
mod tests {
    use super::*;
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
