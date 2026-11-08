use crate::config::{app_dir, write_private};
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, io::Read};
const PREFIX: &str = "01code-attachment:";
fn ref_parts(url: &str) -> Option<(&str, &str)> {
    let (mime, id) = url.strip_prefix(PREFIX)?.split_once(':')?;
    if !matches!(mime, "image/png" | "image/jpeg")
        || id.len() != 64
        || !id.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    Some((mime, id))
}
fn read_bounded(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("图片附件过大".into());
    }
    Ok(bytes)
}
pub fn image_bytes(url: &str) -> Result<Vec<u8>, String> {
    image_bytes_at(url, &app_dir().join("attachments"))
}
fn image_bytes_at(url: &str, directory: &std::path::Path) -> Result<Vec<u8>, String> {
    if let Some((_, id)) = ref_parts(url) {
        let bytes = read_bounded(&directory.join(id))?;
        if format!("{:x}", Sha256::digest(&bytes)) != id {
            return Err("图片附件校验失败".into());
        }
        Ok(bytes)
    } else {
        let (_, data) = url.split_once(";base64,").ok_or("图片数据无效")?;
        if data.len() > 6 * 1024 * 1024 {
            return Err("图片附件过大".into());
        }
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| "图片数据无效".into())
    }
}
pub fn pack(value: &mut Value) -> Result<(), String> {
    pack_at(value, &app_dir().join("attachments"))
}
fn image_urls(
    value: &mut Value,
    visit: &mut impl FnMut(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            for item in items {
                image_urls(item, visit)?;
            }
        }
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("image_url") {
                if let Some(Value::String(url)) =
                    map.get_mut("image_url").and_then(|v| v.get_mut("url"))
                {
                    visit(url)?;
                }
            } else {
                for item in map.values_mut() {
                    image_urls(item, visit)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn pack_at(value: &mut Value, directory: &std::path::Path) -> Result<(), String> {
    image_urls(value, &mut |url| {
        if !url.starts_with("data:image/") {
            return Ok(());
        }
        let (header, encoded) = url.split_once(";base64,").ok_or("图片附件格式无效")?;
        let mime = header
            .strip_prefix("data:")
            .filter(|m| matches!(*m, "image/png" | "image/jpeg"))
            .ok_or("图片格式不受支持")?;
        if encoded.len() > 6 * 1024 * 1024 {
            return Err("图片附件过大".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "图片附件解码失败")?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("图片附件过大".into());
        }
        let id = format!("{:x}", Sha256::digest(&bytes));
        let path = directory.join(&id);
        if read_bounded(&path).map_or(true, |old| old != bytes) {
            write_private(&path, &bytes).map_err(|e| e.to_string())?;
        }
        *url = format!("{PREFIX}{mime}:{id}");
        Ok(())
    })
}
pub fn materialize(value: &mut Value) -> Result<(), String> {
    image_urls(value, &mut |url| {
        if !url.starts_with(PREFIX) {
            return Ok(());
        }
        let (mime, _) = ref_parts(url).ok_or("附件引用无效")?;
        let mime = mime.to_string();
        let bytes = image_bytes(url)?;
        *url = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        Ok(())
    })
}
fn refs(value: &Value, ids: &mut HashSet<String>) {
    match value {
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("image_url") => {
            if let Some(url) = map
                .get("image_url")
                .and_then(|v| v.get("url"))
                .and_then(Value::as_str)
            {
                if let Some((_, id)) = ref_parts(url) {
                    ids.insert(id.into());
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                refs(item, ids);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                refs(item, ids);
            }
        }
        _ => {}
    }
}
pub fn cleanup(history: &Value) -> Result<String, String> {
    cleanup_at(history, &app_dir())
}
fn cleanup_at(history: &Value, directory: &std::path::Path) -> Result<String, String> {
    let mut ids = HashSet::new();
    refs(history, &mut ids);
    // Recovery copies must retain their attachments too.
    let recovery = directory.join("history-recovery.json");
    let mut protect_attachments = false;
    if recovery.exists() {
        if std::fs::metadata(&recovery).is_ok_and(|m| m.len() <= 20 * 1024 * 1024) {
            match std::fs::read(&recovery)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            {
                Some(v) => refs(&v, &mut ids),
                None => protect_attachments = true,
            }
        } else {
            protect_attachments = true;
        }
    }
    let mut bytes = 0u64;
    let mut count = 0;
    let dir = directory.join("attachments");
    if dir.exists() && !protect_attachments {
        for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().map_err(|e| e.to_string())?.is_file()
                && name.len() == 64
                && name.bytes().all(|b| b.is_ascii_hexdigit())
                && !ids.contains(&name)
            {
                let size = e.metadata().map_err(|e| e.to_string())?.len();
                std::fs::remove_file(e.path()).map_err(|e| e.to_string())?;
                bytes += size;
                count += 1;
            }
        }
    }
    let updates = directory.join("updates");
    if updates.exists() {
        for e in std::fs::read_dir(updates).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().map_err(|e| e.to_string())?.is_file()
                && (matches!(
                    name.as_str(),
                    "nullclaw-next" | "coding01-next" | "coding01-next.exe"
                ) || name.starts_with("handoff-")
                    || name.starts_with("01code-download-"))
            {
                let size = e.metadata().map_err(|e| e.to_string())?.len();
                if std::fs::remove_file(e.path()).is_ok() {
                    bytes += size;
                    count += 1;
                }
            }
        }
    }
    Ok(format!(
        "已清理 {count} 个未使用文件，释放 {:.2} MiB",
        bytes as f64 / 1048576.0
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_paths_disguised_as_attachment_refs() {
        assert!(ref_parts("01code-attachment:image/png:../../secret").is_none());
        assert!(ref_parts(&format!("01code-attachment:text/plain:{}", "0".repeat(64))).is_none());
    }
}

pub fn summary() -> String {
    let dir = app_dir();
    let size = |p: std::path::PathBuf| -> u64 {
        if p.is_file() {
            return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        }
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .take(2000)
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum()
    };
    format!(
        "历史 {:.2} MiB · 附件 {:.2} MiB · 网关程序及备份 {:.2} MiB · 更新缓存 {:.2} MiB",
        size(dir.join("history.json")) as f64 / 1048576.0,
        size(dir.join("attachments")) as f64 / 1048576.0,
        size(dir.join("bin")) as f64 / 1048576.0,
        size(dir.join("updates")) as f64 / 1048576.0
    )
}
#[cfg(test)]
mod dedup_tests {
    use super::*;
    #[test]
    fn repeated_images_use_one_file_and_modified_cache_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"image bytes")
        );
        let mut value = serde_json::json!([{"type":"image_url","image_url":{"url":url.clone()}},{"type":"image_url","image_url":{"url":url}}]);
        pack_at(&mut value, dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(value[0], value[1]);
        let reference = value[0]["image_url"]["url"].as_str().unwrap();
        assert_eq!(
            image_bytes_at(reference, dir.path()).unwrap(),
            b"image bytes"
        );
        let (_, id) = ref_parts(reference).unwrap();
        std::fs::write(dir.path().join(id), b"corrupt").unwrap();
        assert!(image_bytes_at(reference, dir.path()).is_err());
        let mut fresh = serde_json::json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(b"image bytes"))}});
        pack_at(&mut fresh, dir.path()).unwrap();
        assert_eq!(
            image_bytes_at(reference, dir.path()).unwrap(),
            b"image bytes"
        );
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;
    #[test]
    fn literal_user_text_is_never_materialized() {
        let dir = tempfile::tempdir().unwrap();
        let mut text =
            serde_json::json!({"role":"user","content":"data:image/png;this is just text"});
        let original = text.clone();
        pack_at(&mut text, dir.path()).unwrap();
        materialize(&mut text).unwrap();
        assert_eq!(text, original);
        let mut text = serde_json::json!({"content":"01code-attachment:image/png:missing"});
        materialize(&mut text).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn cleanup_keeps_recovery_attachments_and_protects_invalid_backup() {
        let root = tempfile::tempdir().unwrap();
        let attachments = root.path().join("attachments");
        std::fs::create_dir(&attachments).unwrap();
        let current = "a".repeat(64);
        let recovered = "b".repeat(64);
        let orphan = "c".repeat(64);
        for id in [&current, &recovered, &orphan] {
            std::fs::write(attachments.join(id), b"fixture").unwrap();
        }
        let block = |id: &str| serde_json::json!({"type":"image_url","image_url":{"url":format!("{PREFIX}image/png:{id}")}});
        std::fs::write(
            root.path().join("history-recovery.json"),
            serde_json::to_vec(&block(&recovered)).unwrap(),
        )
        .unwrap();
        cleanup_at(&block(&current), root.path()).unwrap();
        assert!(attachments.join(&current).exists());
        assert!(attachments.join(&recovered).exists());
        assert!(!attachments.join(&orphan).exists());
        std::fs::write(attachments.join(&orphan), b"fixture").unwrap();
        std::fs::write(root.path().join("history-recovery.json"), b"broken backup").unwrap();
        cleanup_at(&block(&current), root.path()).unwrap();
        assert!(attachments.join(&orphan).exists());
    }
}
