//! 运行环境自检：全平台支持的「底气」从哪来。
//!
//! 解析失败十次里有九次不是链接的问题，而是本机缺东西：没有 ffmpeg、
//! 没有 JS 运行时、没有 Cookie、没有指纹伪装、内核太旧。这里一次性把
//! 这些「为什么不行」摊开，前端只负责显示，判断逻辑都在后端。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::ytdlp;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapKernel {
    pub version: String,
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapTool {
    pub found: bool,
    pub name: Option<String>,
    pub path: Option<String>,
    pub version: Option<String>,
}

impl CapTool {
    fn missing() -> Self {
        Self { found: false, name: None, path: None, version: None }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapImpersonate {
    pub available: bool,
    pub targets: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapBrowser {
    /// yt-dlp 的 --cookies-from-browser 认的名字（chromium / chrome / firefox / …）
    pub browser: String,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapCookies {
    pub detected: Vec<CapBrowser>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapExtractors {
    pub total: usize,
    pub lines: usize,
    pub broken: usize,
    /// 缓存时间（毫秒时间戳）：--list-extractors 要跑一次内核，没必要每次都跑
    pub cached_at: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapDisk {
    pub path: String,
    pub free_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub kernel: Option<CapKernel>,
    pub js_runtime: CapTool,
    pub ffmpeg: CapTool,
    pub impersonate: CapImpersonate,
    pub cookies: CapCookies,
    pub extractors: CapExtractors,
    pub disk: CapDisk,
    pub warnings: Vec<String>,
}

// ── ffmpeg ─────────────────────────────────────────────────────

/// 找出可用的 ffmpeg（PATH 优先，再退常见路径）。合并视频+音频全靠它。
pub fn ffmpeg_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LYCODL_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(p) = which("ffmpeg") {
        return Some(p);
    }
    for p in [
        "/usr/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/snap/bin/ffmpeg",
        "/opt/homebrew/bin/ffmpeg",
    ] {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// ffmpeg 所在目录 —— yt-dlp 的 --ffmpeg-location 要吃目录（或可执行文件），
/// 显式传给内核，免得它在一个干净的 systemd 环境里找不到 PATH 上的 ffmpeg。
pub fn ffmpeg_dir() -> Option<String> {
    let p = ffmpeg_path()?;
    let dir = p.parent()?.display().to_string();
    if dir.trim().is_empty() { None } else { Some(dir) }
}

pub fn which(program: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").ok_or("PATH 未设置")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("{program} 不在 PATH 中"))
}

fn first_line_version(exe: &Path, arg: &str) -> Option<String> {
    let out = Command::new(exe)
        .arg(arg)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim().to_string();
    if line.is_empty() {
        return None;
    }
    // 形如 "ffmpeg version n9.0.2 Copyright …" 或 "v26.13.0"
    if let Some(rest) = line.split_once("version ") {
        return Some(rest.1.split_whitespace().next().unwrap_or(rest.1).to_string());
    }
    Some(line)
}

pub fn ffmpeg() -> CapTool {
    match ffmpeg_path() {
        Some(p) => CapTool {
            found: true,
            name: Some("ffmpeg".into()),
            version: first_line_version(&p, "-version"),
            path: Some(p.display().to_string()),
        },
        None => CapTool::missing(),
    }
}

// ── JS 运行时 ──────────────────────────────────────────────────

pub fn js_runtime() -> CapTool {
    match ytdlp::js_runtime_best() {
        Some((name, path)) => CapTool {
            found: true,
            version: first_line_version(&path, "--version"),
            name: Some(name.to_string()),
            path: Some(path.display().to_string()),
        },
        None => CapTool::missing(),
    }
}

// ── 指纹伪装（curl_cffi）────────────────────────────────────────

struct CachedImpersonate {
    at: Instant,
    value: CapImpersonate,
}

static IMPERSONATE: Mutex<Option<CachedImpersonate>> = Mutex::new(None);

/// 解析 yt-dlp --list-impersonate-targets 的表格。
///
/// 注意：不能拿假目标去试探（--impersonate nope 会直接抛 Python 异常），
/// 只能读这张表。
pub fn impersonate_targets() -> CapImpersonate {
    if let Ok(guard) = IMPERSONATE.lock() {
        if let Some(c) = guard.as_ref() {
            if c.at.elapsed() < Duration::from_secs(3600) {
                return c.value.clone();
            }
        }
    }
    let value = impersonate_targets_uncached();
    if let Ok(mut guard) = IMPERSONATE.lock() {
        *guard = Some(CachedImpersonate { at: Instant::now(), value: value.clone() });
    }
    value
}

fn impersonate_targets_uncached() -> CapImpersonate {
    let kernel = match ytdlp::locate() {
        Ok(k) => k,
        Err(e) => {
            return CapImpersonate { available: false, targets: Vec::new(), error: Some(e) }
        }
    };
    let out = Command::new(&kernel.path)
        .args(["--ignore-config", "--list-impersonate-targets"])
        .stdin(Stdio::null())
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            return CapImpersonate {
                available: false,
                targets: Vec::new(),
                error: Some(format!("执行内核失败: {e}")),
            }
        }
    };
    let mut targets: Vec<String> = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('[') || t.starts_with("Client") || t.starts_with("---") {
            continue;
        }
        let mut parts = t.split_whitespace();
        let client = match parts.next() {
            Some(c) => c,
            None => continue,
        };
        let os = parts.next().unwrap_or("");
        targets.push(if os.is_empty() {
            client.to_string()
        } else {
            format!("{client} {os}")
        });
    }
    if targets.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return CapImpersonate {
            available: false,
            targets: Vec::new(),
            error: Some(if err.is_empty() {
                "内核没有报告任何可用伪装目标（可能缺 curl_cffi）".into()
            } else {
                err
            }),
        };
    }
    CapImpersonate { available: true, targets, error: None }
}

/// 挑一个默认伪装目标：优先桌面 Chrome（最多站点认它），否则取第一个。
pub fn default_impersonate_target() -> Option<String> {
    let cap = impersonate_targets();
    if !cap.available {
        return None;
    }
    let pick = cap
        .targets
        .iter()
        .find(|t| t.starts_with("Chrome-") && !t.contains("Android"))
        .or_else(|| cap.targets.iter().find(|t| t.starts_with("Chrome-")))
        .or_else(|| cap.targets.first());
    pick.map(|t| {
        // --impersonate 只需要 client 名；把 OS 部分去掉
        t.split_whitespace().next().unwrap_or(t).to_string()
    })
}

// ── 本机浏览器（Cookie 来源）────────────────────────────────────

/// 扫已知浏览器的配置目录。目录存在才说明这个浏览器在本机用过。
pub fn detected_browsers() -> Vec<CapBrowser> {
    let home = crate::paths::home_dir();
    let candidates: [(&str, &str, &str); 7] = [
        ("chromium", "Chromium", ".config/chromium"),
        ("chrome", "Google Chrome", ".config/google-chrome"),
        ("edge", "Microsoft Edge", ".config/microsoft-edge"),
        ("brave", "Brave", ".config/BraveSoftware/brave-browser"),
        ("vivaldi", "Vivaldi", ".config/vivaldi"),
        ("opera", "Opera", ".config/opera"),
        ("firefox", "Firefox", ".mozilla/firefox"),
    ];
    let mut out: Vec<CapBrowser> = Vec::new();
    for (browser, label, rel) in candidates {
        let path = home.join(rel);
        if path.is_dir() {
            out.push(CapBrowser {
                browser: browser.to_string(),
                label: label.to_string(),
                path: path.display().to_string(),
            });
        }
    }
    out
}

/// 自动挑一个浏览器取 Cookie：优先 Chromium 系（yt-dlp 支持最好）。
pub fn auto_cookie_browser() -> Option<String> {
    let found = detected_browsers();
    for prefer in ["chromium", "chrome", "edge", "brave", "vivaldi", "firefox", "opera"] {
        if found.iter().any(|b| b.browser == prefer) {
            return Some(prefer.to_string());
        }
    }
    found.first().map(|b| b.browser.clone())
}

// ── 提取器覆盖（--list-extractors）──────────────────────────────

struct CachedExtractors {
    at: Instant,
    at_ms: u64,
    lines: usize,
    names: Vec<(String, bool)>,
    error: Option<String>,
}

static EXTRACTORS: Mutex<Option<CachedExtractors>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn extractors_uncached() -> CachedExtractors {
    let at = Instant::now();
    let at_ms = now_ms();
    let kernel = match ytdlp::locate() {
        Ok(k) => k,
        Err(e) => {
            return CachedExtractors { at, at_ms, lines: 0, names: Vec::new(), error: Some(e) }
        }
    };
    let out = Command::new(&kernel.path)
        .args(["--ignore-config", "--list-extractors"])
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut names: Vec<(String, bool)> = Vec::new();
            let mut lines = 0usize;
            for line in text.lines() {
                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                lines += 1;
                let broken = t.ends_with("(CURRENTLY BROKEN)");
                let name = t.trim_end_matches("(CURRENTLY BROKEN)").trim().to_string();
                if !name.is_empty() {
                    names.push((name, broken));
                }
            }
            names.sort_by_key(|(n, _)| n.to_lowercase());
            let error = if names.is_empty() {
                Some("内核没有输出提取器清单".to_string())
            } else {
                None
            };
            CachedExtractors { at, at_ms, lines, names, error }
        }
        Err(e) => CachedExtractors {
            at,
            at_ms,
            lines: 0,
            names: Vec::new(),
            error: Some(format!("执行内核失败: {e}")),
        },
    }
}

fn extractors_cached() -> CachedExtractors {
    if let Ok(guard) = EXTRACTORS.lock() {
        if let Some(c) = guard.as_ref() {
            if c.at.elapsed() < Duration::from_secs(6 * 3600) {
                return CachedExtractors {
                    at: c.at,
                    at_ms: c.at_ms,
                    lines: c.lines,
                    names: c.names.clone(),
                    error: c.error.clone(),
                };
            }
        }
    }
    let fresh = extractors_uncached();
    if let Ok(mut guard) = EXTRACTORS.lock() {
        *guard = Some(CachedExtractors {
            at: fresh.at,
            at_ms: fresh.at_ms,
            lines: fresh.lines,
            names: fresh.names.clone(),
            error: fresh.error.clone(),
        });
    }
    fresh
}

pub fn extractors() -> CapExtractors {
    let c = extractors_cached();
    let total = c.names.len();
    let broken = c.names.iter().filter(|(_, b)| *b).count();
    CapExtractors {
        total,
        lines: c.lines,
        broken,
        cached_at: Some(c.at_ms),
        error: c.error,
    }
}

/// 供 /api/extractors 搜索用：返回 (名字, 是否上游标注不可用)
pub fn extractor_list() -> Result<Vec<(String, bool)>, String> {
    let c = extractors_cached();
    match c.error {
        Some(e) => Err(e),
        None => Ok(c.names),
    }
}

// ── 磁盘 ───────────────────────────────────────────────────────

pub fn disk(path: &Path) -> CapDisk {
    let (free, total) = statvfs(path);
    CapDisk {
        path: path.display().to_string(),
        free_bytes: free,
        total_bytes: total,
    }
}

#[cfg(unix)]
fn statvfs(path: &Path) -> (Option<u64>, Option<u64>) {
    use std::ffi::CString;
    let c = match CString::new(path.display().to_string()) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return (None, None);
        }
        let frsize = st.f_frsize as u64;
        let bsize = st.f_bsize as u64;
        let unit = if frsize > 0 { frsize } else { bsize };
        let free = (st.f_bavail as u64).saturating_mul(unit);
        let total = (st.f_blocks as u64).saturating_mul(unit);
        (Some(free), Some(total))
    }
}

#[cfg(not(unix))]
fn statvfs(_path: &Path) -> (Option<u64>, Option<u64>) {
    (None, None)
}

// ── 汇总 ───────────────────────────────────────────────────────

pub fn all(output_dir: &Path) -> Capabilities {
    let kernel = ytdlp::locate().ok().map(|k| CapKernel {
        version: k.version.clone(),
        path: k.path.display().to_string(),
        source: match k.source {
            ytdlp::Source::Managed => "managed".to_string(),
            ytdlp::Source::System => "system".to_string(),
        },
    });
    let js = js_runtime();
    let ff = ffmpeg();
    let imp = impersonate_targets();
    let browsers = detected_browsers();
    let ext = extractors();
    let d = disk(output_dir);

    let mut warnings: Vec<String> = Vec::new();
    if kernel.is_none() {
        warnings.push("没找到可用的 yt-dlp 内核：到「设置」点「一键更新内核」。".into());
    }
    if !ff.found {
        warnings.push("没找到 ffmpeg：视频+音频合并会失败，请先安装 ffmpeg。".into());
    }
    if !js.found {
        warnings.push("没找到 JS 运行时（node/deno/bun）：YouTube 等站点的解签名会失败。".into());
    }
    if browsers.is_empty() {
        warnings
            .push("没检测到本机浏览器：需要登录/要 Cookie 的站点（YouTube、抖音、Vimeo…）会解析失败。".into());
    }
    if !imp.available {
        warnings.push("指纹伪装不可用：遇到 Cloudflare 防护的站点只能靠 Cookie 或代理。".into());
    }
    if let Some(f) = d.free_bytes {
        if f < 2 * 1024 * 1024 * 1024 {
            warnings.push(format!(
                "输出目录所在磁盘只剩 {:.1} GB：大文件可能下不完。",
                f as f64 / 1_073_741_824.0
            ));
        }
    }

    Capabilities {
        kernel,
        js_runtime: js,
        ffmpeg: ff,
        impersonate: imp,
        cookies: CapCookies { detected: browsers },
        extractors: ext,
        disk: d,
        warnings,
    }
}
