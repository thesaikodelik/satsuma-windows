//! Background output-size previews for the open wheel. The hovered petal goes first; for
//! light inputs the rest are worked out ahead of time. Results are cached per file state.

use satsuma_core::settings::Settings;
use satsuma_core::WheelItem;
use satsuma_engine::estimate::estimate;
use satsuma_engine::{Cancel, Context, Env, Operation};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

/// The operation a petal runs, if it makes a file whose size can be previewed.
pub fn operation(item: &WheelItem) -> Option<Operation> {
    match item {
        WheelItem::Convert(f) => Some(Operation::Convert(*f)),
        WheelItem::Tool(t) if !t.is_interactive() && !t.has_editor() => Some(Operation::Tool(*t)),
        _ => None,
    }
}

/// What the centre shows for a petal: a size, a short note, or nothing (`None`).
fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Files (with size and modification time, so edits invalidate) plus the operation.
fn key(files: &[PathBuf], op: &Operation, settings: &Settings) -> String {
    let mut k = String::new();
    for f in files {
        let m = std::fs::metadata(f).ok();
        let len = m.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_millis()).unwrap_or(0);
        k.push_str(&format!("{}|{len}|{modified};", f.display()));
    }
    // Compression results depend on the strength settings.
    format!("{k}{op:?}|{:?}{:?}{:?}{:?}", settings.image_compression, settings.image_max_edge, settings.video_compression, settings.audio_compression)
}

pub struct Session {
    priority: Sender<WheelItem>,
    results: Receiver<(WheelItem, Option<String>)>,
    cancel: Cancel,
    last: Option<WheelItem>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Session {
    /// Starts previewing `items` for `files`; with `ahead`, every petal is computed, not only hovered ones.
    pub fn start(env: Arc<Env>, settings: Settings, files: Vec<PathBuf>, items: Vec<WheelItem>, ahead: bool) -> Session {
        let (ptx, prx) = channel::<WheelItem>();
        let (rtx, rrx) = channel();
        let cancel = Cancel::new();
        let job_cancel = cancel.clone();
        std::thread::spawn(move || {
            satsuma_engine::win::ensure_com();
            let ctx = Context { env: &env, settings: &settings, cancel: &job_cancel };
            let mut done: Vec<WheelItem> = Vec::new();
            let mut wanted: Option<WheelItem> = None;
            loop {
                // Take the latest hover; wait only when there is nothing left to do.
                let pending_ahead = ahead && items.iter().any(|i| !done.contains(i) && operation(i).is_some());
                let pending = wanted.is_some_and(|w| !done.contains(&w)) || pending_ahead;
                let next_msg = if pending { prx.try_recv().map_err(|_| RecvTimeoutError::Timeout) } else { prx.recv_timeout(Duration::from_secs(3600)) };
                match next_msg {
                    Ok(item) => {
                        wanted = Some(item);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) if !pending => continue,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if job_cancel.is_cancelled() {
                    return;
                }
                let item = match wanted.filter(|w| !done.contains(w)) {
                    Some(w) => w,
                    None => match items.iter().find(|i| !done.contains(i) && operation(i).is_some()) {
                        Some(i) => *i,
                        None => continue,
                    },
                };
                done.push(item);
                let Some(op) = operation(&item) else {
                    let _ = rtx.send((item, None));
                    continue;
                };
                let k = key(&files, &op, &settings);
                let cached = cache().lock().unwrap().get(&k).cloned();
                let result = match cached {
                    Some(r) => r,
                    None => {
                        let r = match estimate(&ctx, &files, op) {
                            Ok(e) => Some(e.label()),
                            // Compress refuses results that aren't smaller.
                            Err(e) if e.to_string().contains("exceed") => Some("Not smaller".to_string()),
                            Err(_) => None,
                        };
                        if job_cancel.is_cancelled() {
                            return;
                        }
                        cache().lock().unwrap().insert(k, r.clone());
                        r
                    }
                };
                if rtx.send((item, result)).is_err() {
                    return;
                }
            }
        });
        Session { priority: ptx, results: rrx, cancel, last: None }
    }

    /// The pointer is over `item`: work it out next.
    pub fn prioritize(&mut self, item: Option<WheelItem>) {
        if item != self.last {
            self.last = item;
            if let Some(i) = item {
                let _ = self.priority.send(i);
            }
        }
    }

    pub fn poll(&self) -> Vec<(WheelItem, Option<String>)> {
        self.results.try_iter().collect()
    }
}
