//! Bounded local collection of untranslated Japanese UI strings.
use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{
        Mutex,
        mpsc::{self, SyncSender},
    },
};
pub struct Missing {
    sender: Option<SyncSender<String>>,
    seen: Mutex<HashSet<String>>,
}
impl Missing {
    pub fn new(base: &Path, enabled: bool) -> Self {
        let sender = enabled.then(|| {
            let (tx, rx) = mpsc::sync_channel::<String>(256);
            let path = base.join("missing-text.jsonl");
            std::thread::spawn(move || {
                let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
                    return;
                };
                let mut bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
                for text in rx {
                    if bytes > 8 * 1024 * 1024 {
                        break;
                    }
                    if let Ok(line) = serde_json::to_string(&serde_json::json!({"source": text})) {
                        if writeln!(file, "{line}").is_err() {
                            break;
                        }
                        bytes += line.len() as u64 + 1;
                    }
                }
            });
            tx
        });
        Self {
            sender,
            seen: Mutex::new(HashSet::new()),
        }
    }
    pub fn record(&self, text: &str) {
        let Some(sender) = &self.sender else {
            return;
        };
        if text.len() > 4000 || !text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)) {
            return;
        }
        if let Ok(mut seen) = self.seen.try_lock()
            && seen.len() < 5000
            && !seen.contains(text)
            && sender.try_send(text.to_owned()).is_ok()
        {
            seen.insert(text.to_owned());
        }
    }
}
