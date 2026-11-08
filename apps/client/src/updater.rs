use crate::config::{app_dir, write_private};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Product {
    NullClaw,
    Client,
}
impl Product {
    fn repo(self) -> &'static str {
        match self {
            Self::NullClaw => "nullclaw/nullclaw",
            Self::Client => "Cheerimy-Studio/01Code",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::NullClaw => "NullClaw",
            Self::Client => "01Code",
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    pub digest: Option<String>,
}
#[derive(Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    pub assets: Vec<Asset>,
}
pub enum UpdateEvent {
    Release(Release),
    Progress(u64, u64),
    Ready(PathBuf),
    Installed,
    Error(String),
}
pub struct Updater {
    pub product: Product,
    pub release: Option<Release>,
    pub asset: Option<Asset>,
    pub status: String,
    pub busy: bool,
    pub progress: f32,
    pub staged: Option<PathBuf>,
    rx: Option<Receiver<UpdateEvent>>,
    phase: Arc<AtomicU8>,
}
impl Drop for Updater {
    fn drop(&mut self) {
        let _ = self
            .phase
            .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}
impl Updater {
    pub fn new(product: Product) -> Self {
        let status = if product == Product::Client {
            std::fs::read_to_string(app_dir().join("update-result.txt")).unwrap_or_default()
        } else {
            std::fs::read_to_string(app_dir().join("bin/nullclaw.version"))
                .map(|v| format!("本机托管版本：{v}"))
                .unwrap_or_default()
        };
        if product == Product::Client && status == "01Code 更新安装成功" {
            let _ = std::fs::remove_file(app_dir().join("updates").join(if cfg!(windows) {
                "coding01-next.exe"
            } else {
                "coding01-next"
            }));
        }
        Self {
            product,
            release: None,
            asset: None,
            status,
            busy: false,
            progress: 0.0,
            staged: None,
            rx: None,
            phase: Arc::new(AtomicU8::new(3)),
        }
    }
    pub fn check(&mut self) {
        if self.busy {
            return;
        }
        self.phase = Arc::new(AtomicU8::new(1));
        let phase = self.phase.clone();
        self.busy = true;
        self.status = "正在检查 GitHub Releases…".into();
        self.asset = None;
        self.release = None;
        self.staged = None;
        let product = self.product;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let result = if phase.load(Ordering::SeqCst) == 0 {
                UpdateEvent::Error("已取消检查".into())
            } else {
                fetch_release(product)
                    .map(UpdateEvent::Release)
                    .unwrap_or_else(UpdateEvent::Error)
            };
            let _ = tx.send(result);
        });
    }
    pub fn download(&mut self) {
        if self.busy {
            return;
        }
        let Some(asset) = self.asset.clone() else {
            return;
        };
        let product = self.product;
        self.phase = Arc::new(AtomicU8::new(1));
        let phase = self.phase.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.progress = 0.0;
        self.status = "正在下载并校验…".into();
        std::thread::spawn(move || {
            let result = (|| {
                let path = download_asset(product, &asset, &tx, &phase)?;
                if product == Product::NullClaw {
                    install_binary(&path, &managed_nullclaw(), &asset)?;
                    let _ = std::fs::remove_file(path);
                    Ok(UpdateEvent::Installed)
                } else {
                    Ok(UpdateEvent::Ready(path))
                }
            })()
            .unwrap_or_else(UpdateEvent::Error);
            let _ = tx.send(result);
        });
    }
    pub fn can_cancel(&self) -> bool {
        self.busy && self.phase.load(Ordering::SeqCst) == 1
    }
    pub fn cancel(&mut self) {
        if self
            .phase
            .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.status = "正在取消，请稍候…".into();
        }
    }
    pub fn poll(&mut self) {
        let events = self
            .rx
            .as_ref()
            .map(|r| r.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for event in events {
            match event {
                UpdateEvent::Release(r) => {
                    self.busy = false;
                    if self.phase.load(Ordering::SeqCst) == 0 {
                        self.status = "已取消检查".into();
                        continue;
                    }
                    self.asset = select_asset(self.product, &r.assets);
                    self.status = if self.product == Product::Client
                        && !is_newer(&r.tag_name, env!("CARGO_PKG_VERSION"))
                    {
                        self.asset = None;
                        "当前已是最新版本，或发布标签无法识别".into()
                    } else if self.asset.is_some() {
                        format!("可用版本 {}", r.tag_name)
                    } else {
                        "该发布没有当前平台的可校验原始二进制，请在发布页手动安装".into()
                    };
                    if self.product == Product::NullClaw
                        && managed_nullclaw().is_file()
                        && std::fs::read_to_string(app_dir().join("bin/nullclaw.version"))
                            .is_ok_and(|v| v == r.tag_name)
                    {
                        self.asset = None;
                        self.status = format!("托管版本 {} 已是最新", r.tag_name);
                    }
                    self.release = Some(r);
                }
                UpdateEvent::Progress(done, total) => {
                    if self.phase.load(Ordering::SeqCst) == 0 {
                        continue;
                    }
                    self.progress = done as f32 / total.max(1) as f32;
                    self.status = format!(
                        "下载中 {:.1} / {:.1} MiB",
                        done as f64 / 1048576.0,
                        total as f64 / 1048576.0
                    );
                }
                UpdateEvent::Ready(p) => {
                    self.staged = Some(p);
                    self.busy = false;
                    self.status = "下载和 SHA-256 校验完成，可安装并重启".into();
                    self.progress = 1.0;
                }
                UpdateEvent::Installed => {
                    if let Some(release) = &self.release {
                        let _ = write_private(
                            &app_dir().join("bin/nullclaw.version"),
                            release.tag_name.as_bytes(),
                        );
                    }
                    self.busy = false;
                    self.status = "NullClaw 已安装到 01Code 管理的目录，请启动网关".into();
                    self.progress = 1.0;
                }
                UpdateEvent::Error(e) => {
                    self.busy = false;
                    self.status = if self.phase.load(Ordering::SeqCst) == 0 {
                        "已取消，当前程序未被替换".into()
                    } else {
                        e
                    };
                }
            }
        }
    }
    pub fn apply_client_update(&mut self) -> Result<(), String> {
        let staged = self.staged.clone().ok_or("请先下载更新")?;
        let asset = self.asset.as_ref().ok_or("更新资源不存在")?;
        verify_file(&staged, asset)?;
        let target = std::env::current_exe().map_err(|e| e.to_string())?;
        let handoff = Handoff {
            parent: std::process::id(),
            target,
            staged: staged.clone(),
            asset: asset.clone(),
        };
        let path = app_dir()
            .join("updates")
            .join(format!("handoff-{}.json", uuid::Uuid::new_v4()));
        write_private(
            &path,
            &serde_json::to_vec(&handoff).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new(staged);
        cmd.arg("--01code-apply-update").arg(&path);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        cmd.spawn().map_err(|e| format!("启动更新程序失败：{e}"))?;
        Ok(())
    }
}
fn github_client() -> Result<reqwest::blocking::Client, String> {
    let builder = reqwest::Client::builder()
        .user_agent(concat!("01Code/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(5))
        .read_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() < 5
                && url.scheme() == "https"
                && matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                            | "github-releases.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }));
    reqwest::blocking::ClientBuilder::from(builder)
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())
}
fn fetch_release(product: Product) -> Result<Release, String> {
    let response = github_client()?
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            product.repo()
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| "无法连接 GitHub，请检查网络".to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("仓库尚无可用 Release；源码提交和 Actions 构建不会被当作可安装更新".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "GitHub 返回 HTTP {}，可能受限流影响",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("发布信息过大".into());
    }
    let r: Release = serde_json::from_slice(&bytes).map_err(|_| "GitHub 发布信息无效")?;
    if !r
        .html_url
        .starts_with(&format!("https://github.com/{}/releases/", product.repo()))
    {
        return Err("发布页来源不匹配".into());
    }
    Ok(r)
}
fn expected_names(product: Product, os: &str, arch: &str) -> Vec<String> {
    let os = if os == "macos" { "macos" } else { os };
    let arch = if arch == "aarch64" { "aarch64" } else { arch };
    if !matches!(os, "windows" | "linux" | "macos") || !matches!(arch, "x86_64" | "aarch64") {
        return Vec::new();
    }
    let ext = if os == "windows" { "exe" } else { "bin" };
    let prefix = if product == Product::NullClaw {
        "nullclaw"
    } else {
        "coding01"
    };
    let mut names = vec![format!("{prefix}-{os}-{arch}.{ext}")];
    if product == Product::Client {
        names.push(format!("01code-{os}-{arch}.{ext}"));
    }
    names
}
fn select_asset(product: Product, assets: &[Asset]) -> Option<Asset> {
    let names = expected_names(product, std::env::consts::OS, std::env::consts::ARCH);
    names.iter().find_map(|name| {
        assets
            .iter()
            .find(|a| &a.name == name && valid_digest(a.digest.as_deref()))
            .cloned()
    })
}
fn valid_digest(digest: Option<&str>) -> bool {
    digest
        .and_then(|s| s.strip_prefix("sha256:"))
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn download_asset(
    product: Product,
    asset: &Asset,
    tx: &mpsc::Sender<UpdateEvent>,
    phase: &AtomicU8,
) -> Result<PathBuf, String> {
    if phase.load(Ordering::SeqCst) == 0 {
        return Err("已取消下载".into());
    }
    if !asset.browser_download_url.starts_with(&format!(
        "https://github.com/{}/releases/download/",
        product.repo()
    )) {
        return Err("下载地址不属于官方仓库".into());
    }
    if !valid_digest(asset.digest.as_deref()) || asset.size == 0 || asset.size > 64 * 1024 * 1024 {
        return Err("更新包缺少有效校验值或大小不符合限制".into());
    }
    let dir = app_dir().join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut temp = tempfile::Builder::new()
        .prefix("01code-download-")
        .tempfile_in(&dir)
        .map_err(|e| e.to_string())?;
    let mut response = github_client()?
        .get(&asset.browser_download_url)
        .send()
        .map_err(|_| "下载更新失败")?;
    if !response.status().is_success() {
        return Err(format!("下载返回 HTTP {}", response.status()));
    }
    let mut received = 0;
    let mut buf = [0u8; 64 * 1024];
    let mut last = std::time::Instant::now();
    loop {
        if phase.load(Ordering::SeqCst) == 0 {
            return Err("已取消下载".into());
        }
        let n = response.read(&mut buf).map_err(|_| "下载中断")?;
        if n == 0 {
            break;
        }
        received += n as u64;
        if received > asset.size {
            return Err("下载大小超过发布声明".into());
        }
        temp.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        if last.elapsed().as_millis() >= 200 {
            let _ = tx.send(UpdateEvent::Progress(received, asset.size));
            last = std::time::Instant::now();
        }
    }
    if received != asset.size {
        return Err("下载不完整".into());
    }
    let destination = dir.join(if product == Product::Client {
        if cfg!(windows) {
            "coding01-next.exe"
        } else {
            "coding01-next"
        }
    } else {
        "nullclaw-next"
    });
    finish_download(temp, &destination, asset, phase)?;
    Ok(destination)
}
fn finish_download(
    temp: tempfile::NamedTempFile,
    destination: &Path,
    asset: &Asset,
    phase: &AtomicU8,
) -> Result<(), String> {
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    verify_file(temp.path(), asset)?;
    phase
        .compare_exchange(1, 2, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "已取消下载")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    temp.persist(destination).map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_file(path: &Path, asset: &Asset) -> Result<(), String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if f.metadata().map_err(|e| e.to_string())?.len() != asset.size {
        return Err("文件大小校验失败".into());
    }
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut magic = Vec::new();
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if magic.is_empty() {
            magic.extend_from_slice(&buf[..n.min(4)]);
        }
        hasher.update(&buf[..n]);
    }
    if format!("sha256:{:x}", hasher.finalize())
        != asset
            .digest
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
    {
        return Err("SHA-256 校验失败，已阻止安装".into());
    }
    let executable = if cfg!(windows) {
        magic.starts_with(b"MZ")
    } else if cfg!(target_os = "macos") {
        matches!(
            magic.as_slice(),
            [0xcf, 0xfa, 0xed, 0xfe] | [0xfe, 0xed, 0xfa, 0xcf] | [0xca, 0xfe, 0xba, 0xbe]
        )
    } else {
        magic == b"\x7fELF"
    };
    if !executable {
        return Err("发布资源不是当前平台的原始可执行文件".into());
    }
    Ok(())
}
pub fn managed_nullclaw() -> PathBuf {
    app_dir().join("bin").join(if cfg!(windows) {
        "nullclaw.exe"
    } else {
        "nullclaw"
    })
}
fn install_binary(staged: &Path, target: &Path, asset: &Asset) -> Result<(), String> {
    verify_file(staged, asset)?;
    let parent = target.parent().ok_or("目标目录无效")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    std::io::copy(
        &mut std::fs::File::open(staged).map_err(|e| e.to_string())?,
        temp.as_file_mut(),
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    if target.is_file() {
        let mut backup = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        std::io::copy(
            &mut std::fs::File::open(target).map_err(|e| e.to_string())?,
            backup.as_file_mut(),
        )
        .map_err(|e| format!("备份旧版本失败：{e}"))?;
        backup.as_file().sync_all().map_err(|e| e.to_string())?;
        backup
            .persist(target.with_extension("previous"))
            .map_err(|e| format!("保存旧版本失败：{e}"))?;
    }
    temp.persist(target)
        .map_err(|e| format!("替换程序失败，请检查目录权限或程序是否仍在运行：{e}"))?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Handoff {
    parent: u32,
    target: PathBuf,
    staged: PathBuf,
    asset: Asset,
}
/// Called before initializing the GUI by the verified staged client.
pub fn handle_update_cli() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|a| a != "--01code-apply-update") {
        return false;
    }
    let mut fallback = None;
    let result = (|| {
        let path = PathBuf::from(args.get(2).ok_or("更新交接文件缺失")?);
        let allowed = app_dir()
            .join("updates")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if path.canonicalize().map_err(|e| e.to_string())?.parent() != Some(allowed.as_path()) {
            return Err("更新交接目录无效".into());
        }
        let h = read_handoff(&path, &allowed)?;
        if std::env::current_exe()
            .map_err(|e| e.to_string())?
            .canonicalize()
            .ok()
            != h.staged.canonicalize().ok()
        {
            return Err("更新程序路径不匹配".into());
        }
        wait_parent(h.parent)?;
        fallback = Some(h.target.clone());
        install_binary(&h.staged, &h.target, &h.asset)?;
        write_private(
            &app_dir().join("update-result.txt"),
            "01Code 更新安装成功".as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        std::process::Command::new(&h.target)
            .spawn()
            .map_err(|e| format!("更新成功，但自动重启失败：{e}"))?;
        let _ = std::fs::remove_file(path);
        Ok::<(), String>(())
    })();
    if let Err(e) = result {
        let _ = write_private(&app_dir().join("update-result.txt"), e.as_bytes());
        if let Some(target) = fallback {
            let _ = std::process::Command::new(target).spawn();
        }
    }
    true
}
fn read_handoff(path: &Path, allowed: &Path) -> Result<Handoff, String> {
    if path.canonicalize().map_err(|e| e.to_string())?.parent() != Some(allowed) {
        return Err("更新交接目录无效".into());
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 {
        return Err("更新交接文件过大".into());
    }
    let h: Handoff = serde_json::from_slice(&bytes).map_err(|_| "更新交接格式无效")?;
    let source = h.staged.canonicalize().map_err(|e| e.to_string())?;
    if source.parent() != Some(allowed)
        || source == h.target.canonicalize().map_err(|e| e.to_string())?
    {
        return Err("更新来源或目标路径无效".into());
    }
    if h.target.extension().is_some_and(|e| e == "previous")
        || h.parent == 0
        || h.parent > i32::MAX as u32
    {
        return Err("更新目标或父进程无效".into());
    }
    if !h
        .asset
        .browser_download_url
        .starts_with("https://github.com/Cheerimy-Studio/01Code/releases/download/")
        || !expected_names(
            Product::Client,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
        .contains(&h.asset.name)
    {
        return Err("更新资源来源不匹配".into());
    }
    verify_file(&h.staged, &h.asset)?;
    Ok(h)
}
fn wait_parent(pid: u32) -> Result<(), String> {
    if pid == 0 || pid > i32::MAX as u32 || pid == std::process::id() {
        return Err("更新父进程无效".into());
    }
    #[cfg(unix)]
    {
        let start = std::time::Instant::now();
        while unsafe { libc::kill(pid as i32, 0) } == 0 {
            if start.elapsed().as_secs() > 20 {
                return Err("原程序尚未退出，更新未执行".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
            fn WaitForSingleObject(handle: isize, timeout: u32) -> u32;
            fn CloseHandle(handle: isize) -> i32;
        }
        let handle = OpenProcess(0x00100000, 0, pid);
        if handle == 0 {
            return Ok(());
        }
        let result = WaitForSingleObject(handle, 20000);
        CloseHandle(handle);
        if result == 0 {
            Ok(())
        } else {
            Err("原程序尚未退出，更新未执行".into())
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err("当前平台不支持自动更新".into())
    }
}
fn is_newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u64>> {
        let s = s.strip_prefix('v').unwrap_or(s);
        if s.contains(['-', '+']) {
            return None;
        }
        let parts = s
            .split('.')
            .map(str::parse)
            .collect::<Result<Vec<u64>, _>>()
            .ok()?;
        if parts.len() != 3 {
            return None;
        }
        Some(parts)
    };
    matches!((parse(tag),parse(current)),(Some(a),Some(b)) if a>b)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_installs_cross_archives() {
        assert_eq!(
            expected_names(Product::NullClaw, "windows", "x86_64"),
            vec!["nullclaw-windows-x86_64.exe"]
        );
        assert!(expected_names(Product::Client, "linux", "arm").is_empty());
    }
    #[test]
    fn release_versions_are_compared_numerically() {
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(!is_newer("main", "0.2.0"));
    }
    #[test]
    fn hash_mismatch_cannot_replace_working_binary() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("staged");
        let target = dir.path().join("client");
        std::fs::write(&staged, b"bad").unwrap();
        std::fs::write(&target, b"working").unwrap();
        let a = Asset {
            name: "client".into(),
            browser_download_url: String::new(),
            size: 3,
            digest: Some(format!("sha256:{}", "0".repeat(64))),
        };
        assert!(install_binary(&staged, &target, &a).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"working");
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;
    #[test]
    fn verified_binary_install_keeps_one_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("next");
        let target = dir.path().join("current");
        let bytes = if cfg!(windows) {
            b"MZfixture".to_vec()
        } else if cfg!(target_os = "macos") {
            vec![0xcf, 0xfa, 0xed, 0xfe, 1, 2]
        } else {
            b"\x7fELFfixture".to_vec()
        };
        std::fs::write(&staged, &bytes).unwrap();
        std::fs::write(&target, b"old").unwrap();
        let a = Asset {
            name: String::new(),
            browser_download_url: String::new(),
            size: bytes.len() as u64,
            digest: Some(format!("sha256:{:x}", Sha256::digest(&bytes))),
        };
        install_binary(&staged, &target, &a).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert_eq!(
            std::fs::read(target.with_extension("previous")).unwrap(),
            b"old"
        );
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    fn fixture() -> (Vec<u8>, Asset) {
        let bytes = if cfg!(windows) {
            b"MZfixture".to_vec()
        } else if cfg!(target_os = "macos") {
            vec![0xcf, 0xfa, 0xed, 0xfe, 1, 2]
        } else {
            b"\x7fELFfixture".to_vec()
        };
        let asset = Asset {
            name: expected_names(
                Product::Client,
                std::env::consts::OS,
                std::env::consts::ARCH,
            )[0]
            .clone(),
            browser_download_url:
                "https://github.com/Cheerimy-Studio/01Code/releases/download/v1.0.0/fixture".into(),
            size: bytes.len() as u64,
            digest: Some(format!("sha256:{:x}", Sha256::digest(&bytes))),
        };
        (bytes, asset)
    }
    #[test]
    fn cancelled_verified_download_cannot_replace_staged_binary() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("next");
        std::fs::write(&destination, b"previous-stage").unwrap();
        let (bytes, asset) = fixture();
        let mut temp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        temp.write_all(&bytes).unwrap();
        assert!(finish_download(temp, &destination, &asset, &AtomicU8::new(0)).is_err());
        assert_eq!(std::fs::read(destination).unwrap(), b"previous-stage");
    }
    #[test]
    fn late_release_is_ignored_after_cancel_and_busy_clears_only_on_completion() {
        let mut updater = Updater::new(Product::Client);
        let (tx, rx) = mpsc::channel();
        updater.rx = Some(rx);
        updater.phase = Arc::new(AtomicU8::new(1));
        updater.busy = true;
        updater.cancel();
        assert!(updater.busy);
        assert!(!updater.can_cancel());
        tx.send(UpdateEvent::Release(Release {
            tag_name: "v99.0.0".into(),
            html_url: String::new(),
            body: None,
            assets: vec![fixture().1],
        }))
        .unwrap();
        updater.poll();
        assert!(!updater.busy);
        assert!(updater.asset.is_none());
        assert!(updater.release.is_none());
    }
    #[test]
    fn handoff_rejects_outside_source_and_same_target() {
        let dir = tempfile::tempdir().unwrap();
        let allowed = dir.path().join("updates");
        std::fs::create_dir(&allowed).unwrap();
        let allowed = allowed.canonicalize().unwrap();
        let path = allowed.join("handoff.json");
        let (bytes, asset) = fixture();
        let target = dir.path().join("client");
        std::fs::write(&target, b"old").unwrap();
        let outside = dir.path().join("outside");
        std::fs::write(&outside, &bytes).unwrap();
        let mut h = Handoff {
            parent: 42,
            target: target.clone(),
            staged: outside,
            asset,
        };
        std::fs::write(&path, serde_json::to_vec(&h).unwrap()).unwrap();
        assert!(read_handoff(&path, &allowed).is_err());
        h.staged = allowed.join("next");
        std::fs::write(&h.staged, &bytes).unwrap();
        std::fs::write(&path, serde_json::to_vec(&h).unwrap()).unwrap();
        assert!(read_handoff(&path, &allowed).is_ok());
        h.target = h.staged.clone();
        std::fs::write(&path, serde_json::to_vec(&h).unwrap()).unwrap();
        assert!(read_handoff(&path, &allowed).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"old");
    }
}
