use crate::model::{Item, Session};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        mpsc::{sync_channel, SyncSender},
        OnceLock,
    },
};
#[derive(Serialize, Deserialize)]
struct History {
    sessions: Vec<Session>,
    transcripts: HashMap<String, Vec<Item>>,
    active: String,
}
pub fn load() -> Option<(Vec<Session>, HashMap<String, Vec<Item>>, String)> {
    let f = std::fs::File::open(crate::config::data_dir().join("history.json")).ok()?;
    let mut bytes = Vec::new();
    f.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }
    let mut h: History = serde_json::from_slice(&bytes).ok()?;
    h.sessions.truncate(100);
    h.transcripts
        .retain(|id, _| h.sessions.iter().any(|s| &s.sid == id));
    for items in h.transcripts.values_mut() {
        for item in items {
            match item {
                Item::Ai(s) => {
                    s.done = true;
                    s.typed = s.text.chars().count();
                }
                Item::Approval(a) if a.responded.is_none() => a.responded = Some(false),
                _ => {}
            }
        }
    }
    if !h.sessions.iter().any(|s| s.sid == h.active) {
        h.active = h.sessions.first()?.sid.clone();
    }
    Some((h.sessions, h.transcripts, h.active))
}
fn snapshot(model: &crate::model::Model) -> serde_json::Value {
    let mut sessions = model.sessions.clone();
    sessions.truncate(100);
    let transcripts = sessions
        .iter()
        .filter_map(|s| {
            model
                .transcripts
                .get(&s.sid)
                .map(|items| (s.sid.clone(), items.clone()))
        })
        .collect();
    serde_json::to_value(History {
        sessions,
        transcripts,
        active: model.active_sid.clone(),
    })
    .unwrap_or(serde_json::Value::Null)
}
fn write(path: &std::path::Path, v: &serde_json::Value) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap();
    let _ = crate::config::private_write(path, v);
}
fn sender() -> &'static SyncSender<serde_json::Value> {
    static S: OnceLock<SyncSender<serde_json::Value>> = OnceLock::new();
    S.get_or_init(|| {
        let (tx, rx) = sync_channel(1);
        let path = crate::config::data_dir().join("history.json");
        std::thread::spawn(move || {
            while let Ok(v) = rx.recv() {
                if serde_json::to_vec(&v)
                    .map(|b| b.len() <= 16 * 1024 * 1024)
                    .unwrap_or(false)
                {
                    write(&path, &v);
                }
            }
        });
        tx
    })
}
pub fn save(model: &mut crate::model::Model) {
    if model.gateway_mode && model.history_dirty {
        match sender().try_send(snapshot(model)) {
            Ok(()) => model.history_dirty = false,
            Err(_) => {}
        }
    }
}
pub fn flush(model: &crate::model::Model) {
    if model.gateway_mode {
        let v = snapshot(model);
        if serde_json::to_vec(&v)
            .map(|b| b.len() <= 16 * 1024 * 1024)
            .unwrap_or(false)
        {
            write(&crate::config::data_dir().join("history.json"), &v);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_stores_text_without_textures() {
        let item = Item::User(crate::model::UserMsg {
            text: "hello".into(),
            time: "12:00".into(),
            images: Vec::new(),
        });
        let value = serde_json::to_value(&item).unwrap();
        let restored: Item = serde_json::from_value(value).unwrap();
        assert!(matches!(restored,Item::User(m) if m.text=="hello"&&m.images.is_empty()));
    }
    #[test]
    fn real_sessions_do_not_reset_history() {
        let mut m = crate::model::Model::new(false);
        m.new_task();
        let a = m.active_sid.clone();
        m.input = "first".into();
        m.user_submit();
        m.new_task();
        let b = m.active_sid.clone();
        assert_ne!(a, b);
        assert!(!m.transcripts[&a].is_empty());
        m.input = "second".into();
        m.user_submit();
        assert_eq!(m.active_sid, b);
        assert_eq!(m.drain_outbox().len(), 2);
    }
}
