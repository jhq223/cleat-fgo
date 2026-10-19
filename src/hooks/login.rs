//! Save the game-provided login response for Chaldea.
use crate::{APP, update::atomic_write};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use cleat::prelude::*;
use std::sync::{
    OnceLock,
    mpsc::{SyncSender, sync_channel},
};

const MAX_BODY: usize = 32 * 1024 * 1024;
static WRITER: OnceLock<SyncSender<String>> = OnceLock::new();

#[cleat::hook("DelightWorks.Network.UnityWebRequestWWW", "get_text")]
pub fn response_text(this: &Object) -> Result<Il2CppString> {
    let response = response_text::original(this)?;
    let Some(app) = APP.get().filter(|app| app.config.export_login_response) else {
        return Ok(response);
    };
    if response.is_null() || response.len() > MAX_BODY / 2 {
        return Ok(response);
    }
    let Some(body) = decode_body(response.to_string_lossy()) else {
        return Ok(response);
    };
    // EndTopLogin receives only the success fragment. Here the game still has
    // the complete response body, including the cache Chaldea needs.
    if ![
        "\"login\"",
        "\"toplogin\"",
        "\"top/login\"",
        "\"login/top\"",
    ]
    .iter()
    .any(|marker| body.contains(marker))
    {
        return Ok(response);
    }
    let sender = WRITER.get_or_init(|| {
        let (sender, receiver) = sync_channel::<String>(1);
        let path = app.base.join("exports/chaldea-login.json");
        std::thread::spawn(move || {
            for body in receiver {
                match validate_response(&body).and_then(|()| save_response(&path, &body)) {
                    Ok(()) => log::info!(
                        "Chaldea login response saved: {} ({} bytes)",
                        path.display(),
                        body.len()
                    ),
                    Err(error) => {
                        log::warn!("Chaldea login export: {error}")
                    }
                }
            }
        });
        sender
    });
    if sender.try_send(body).is_err() {
        log::warn!("Chaldea login export is busy; this response was not queued");
    }
    Ok(response)
}

fn save_response(path: &std::path::Path, body: &str) -> std::result::Result<(), String> {
    atomic_write(path, body.as_bytes())?;
    #[cfg(target_os = "android")]
    if crate::android::export_login(body.as_bytes())? {
        log::info!("Chaldea export available: Download/cleat-fgo/chaldea-login.json");
    }
    Ok(())
}

fn decode_body(body: String) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        return Some(body);
    }
    // The JP endpoint may return a base64-encoded JSON response. Decode only
    // JSON candidates; binary downloads and other responses are not exported.
    if !trimmed.starts_with("ey") && !trimmed.starts_with("ew") {
        return None;
    }
    let bytes = STANDARD.decode(trimmed).ok()?;
    (bytes.len() <= MAX_BODY)
        .then(|| String::from_utf8(bytes).ok())
        .flatten()
}

fn validate_response(body: &str) -> std::result::Result<(), String> {
    if body.len() > MAX_BODY {
        return Err("response exceeds export size limit".into());
    }
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "login callback did not contain a JSON response".to_owned())?;
    let success = value
        .get("response")
        .and_then(|v| v.as_array())
        .is_some_and(|items| {
            items.iter().any(|entry| {
                matches!(
                    entry.get("nid").and_then(|v| v.as_str()),
                    Some("login" | "toplogin" | "top/login" | "login/top")
                ) && entry.get("success").is_some_and(|v| v.is_object())
                    && matches!(
                        entry.get("resCode").and_then(|v| v.as_str()),
                        Some("00" | "0")
                    )
            })
        });
    if !success || !value.get("cache").is_some_and(|v| v.is_object()) {
        let kind = |v: Option<&serde_json::Value>| match v {
            Some(serde_json::Value::Object(_)) => "object",
            Some(serde_json::Value::Array(_)) => "array",
            Some(serde_json::Value::String(_)) => "string",
            None => "absent",
            _ => "other",
        };
        return Err(format!(
            "login response has no successful account cache (response={}, cache={}, login={:?})",
            kind(value.get("response")),
            kind(value.get("cache")),
            value["response"].as_array().map(|items| items
                .iter()
                .filter(|entry| entry["nid"].as_str() == Some("login"))
                .map(|entry| (
                    kind(entry.get("success")),
                    kind(entry.get("fail")),
                    entry
                        .get("resCode")
                        .and_then(|v| v.as_str())
                        .filter(|s| s.len() < 8)
                ))
                .collect::<Vec<_>>())
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_login_responses_replace_the_export() {
        let json = r#"{"response":[{"nid":"toplogin","resCode":"00","success":{}}],"cache":{}}"#;
        assert_eq!(decode_body(STANDARD.encode(json)).as_deref(), Some(json));
        assert!(decode_body("not a response".into()).is_none());
        assert!(validate_response(r#"{"response":[{"nid":"toplogin","resCode":"00","success":{}}],"cache":{"updated":{"userSvt":[]}}}"#).is_ok());
        assert!(validate_response(r#"{"response":[{"nid":"login","resCode":"00","success":{},"fail":{}},{"nid":"userdata","success":{}}],"cache":{"updated":{"userSvt":[]},"replaced":{}}}"#).is_ok());
        for bad in [
            r#"{"response":[{"nid":"home","success":{}}],"cache":{}}"#,
            r#"{"response":[{"nid":"login","resCode":"10","success":{},"fail":{"message":"failed"}}],"cache":{}}"#,
            "",
            "success",
            "{}",
            r#"{"response":[{"fail":{}}],"cache":{}}"#,
            r#"{"response":[{"success":{}}]}"#,
        ] {
            assert!(validate_response(bad).is_err());
        }
    }
}
