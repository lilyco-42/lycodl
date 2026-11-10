//! yt-dlp 内核的所有权：定位、版本、解析、下载、热更新。
//!
//! 设计要点（对应「cli 可以热更新，为了获取 cli 的更新红利」）：
//! - 内核是外部进程，不是编译进二进制的库 —— 换内核不需要重编 lycodl。
//! - lycodl 自己管理一份内核（data_dir/bin/yt-dlp），优先于系统那份，
//!   这样即使系统包管理器装的是旧版，也能一键升级到上游最新。
//! - 上游 GitHub Release 自带 SHA2-256SUMS，下载后强制校验再原子替换。

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const GITHUB_LATEST: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
pub const GITHUB_DL_PREFIX: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Managed,
    System,
}

#[derive(Debug, Clone)]
pub struct Kernel {
    pub path: PathBuf,
    pub version: String,
    pub source: Source,
}

/// 在若干候选里找出可用的 yt-dlp（能跑出 --version 才算数）
pub fn locate() -> Result<Kernel, String> {
    let mut candidates: Vec<(PathBuf, Source)> = Vec::new();
    if let Some(p) = std::env::var_os("LYCODL_YTDLP") {
        candidates.push((PathBuf::from(p), Source::Managed));
    }
    let managed = crate::paths::managed_ytdlp();
    candidates.push((managed.clone(), Source::Managed));
    if let Ok(p) = which("yt-dlp") {
        candidates.push((p, Source::System));
    }
    candidates.push((PathBuf::from("/usr/bin/yt-dlp"), Source::System));
    candidates.push((
        crate::paths::home_dir().join(".local/bin/yt-dlp"),
        Source::System,
    ));

    let mut first_error: Option<String> = None;
    for (path, source) in candidates {
        if !path.is_file() {
            continue;
        }
        match version_of(&path) {
            Ok(version) => {
                return Ok(Kernel {
                    path,
                    version,
                    source,
                })
            }
            Err(e) => {
                first_error.get_or_insert(format!("{}: {e}", path.display()));
            }
        }
    }
    Err(match first_error {
        Some(e) => format!("没找到可用的 yt-dlp（{e}）"),
        None => "没找到 yt-dlp：请安装 yt-dlp，或在设置页点「一键更新内核」由 lycodl 自己管理".into(),
    })
}

// -- JavaScript 运行时 ----------------------------------------
//
// 2026 年起 YouTube 的 nsig / SABR 处理必须借助 JS 运行时解签名，
// yt-dlp 默认一个都不启用（--verbose 里能看到 "JS runtimes: none"），
// 结果是「No video formats found!」。这里替它把本机的 node / deno / bun
// 找出来，用 --js-runtimes 显式喂进去。

fn js_runtime_candidates() -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = Vec::new();
    for name in ["node", "deno", "bun"] {
        if let Ok(p) = which(name) {
            out.push((name, p));
        }
    }
    // nvm / fnm / volta 装的 node 通常不在 PATH 中（GUI 进程尤其如此）
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        let nvm = home.join(".nvm/versions/node");
        if let Ok(rd) = std::fs::read_dir(&nvm) {
            let mut vers: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
            vers.sort();
            if let Some(last) = vers.last() {
                let bin = last.join("bin/node");
                if bin.is_file() {
                    out.push(("node", bin));
                }
            }
        }
        for p in [
            home.join(".volta/bin/node"),
            home.join(".fnm/aliases/default/bin/node"),
            home.join(".bun/bin/bun"),
        ] {
            if p.is_file() {
                let name = if p.file_name().and_then(|n| n.to_str()) == Some("bun") {
                    "bun"
                } else {
                    "node"
                };
                out.push((name, p));
            }
        }
    }
    out
}

/// 生成 --js-runtimes name:path 参数（找不到运行时则返回空，交给 yt-dlp 决定）
pub fn js_runtime_args() -> Vec<String> {
    if let Ok(v) = std::env::var("LYCODL_JS_RUNTIME") {
        let v = v.trim();
        if !v.is_empty() {
            return vec!["--js-runtimes".into(), v.to_string()];
        }
    }
    match js_runtime_candidates().into_iter().next() {
        Some((name, path)) => vec!["--js-runtimes".into(), format!("{name}:{}", path.display())],
        None => Vec::new(),
    }
}

fn which(program: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").ok_or("PATH 未设置")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("{program} 不在 PATH 中"))
}

/// 读某个 yt-dlp 的版本（跑 --version，失败即视为不可用）
pub fn version_of(exe: &Path) -> Result<String, String> {
    let out = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("执行失败: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("退出码非零: {}", err.trim()));
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        return Err("版本输出为空".into());
    }
    Ok(v)
}

// ── 解析（-J）────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatInfo {
    pub format_id: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub height: Option<i64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub filesize: Option<i64>,
    pub tbr: Option<f64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub title: String,
    pub url: String,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub webpage_url: String,
    pub extractor: String,
    pub is_playlist: bool,
    pub entry_count: usize,
    pub entries: Vec<PlaylistEntry>,
    pub formats: Vec<FormatInfo>,
}

fn s(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn f(v: &serde_json::Value, k: &str) -> Option<f64> {
    v.get(k).and_then(|x| x.as_f64())
}

fn i(v: &serde_json::Value, k: &str) -> Option<i64> {
    v.get(k)
        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|y| y as i64)))
}

fn entry_url(v: &serde_json::Value) -> Option<String> {
    s(v, "webpage_url")
        .or_else(|| s(v, "url"))
        .or_else(|| {
            let id = s(v, "id")?;
            let ie = v
                .get("ie_key")
                .and_then(|x| x.as_str())
                .unwrap_or("Youtube");
            Some(format!("https://www.youtube.com/watch?v={id}").replace("Youtube", ie).to_string())
        })
}

/// 把 yt-dlp -J 的 JSON 归一化成前端契约里的 VideoInfo
pub fn parse_info(root: &serde_json::Value) -> VideoInfo {
    let is_playlist = root
        .get("_type")
        .and_then(|x| x.as_str())
        .map(|t| t == "playlist" || t == "multi_video")
        .unwrap_or_else(|| root.get("entries").is_some());

    let mut entries: Vec<PlaylistEntry> = Vec::new();
    if let Some(arr) = root.get("entries").and_then(|x| x.as_array()) {
        for e in arr.iter().take(500) {
            if e.is_null() {
                continue;
            }
            let url = match entry_url(e) {
                Some(u) => u,
                None => continue,
            };
            entries.push(PlaylistEntry {
                title: s(e, "title").unwrap_or_else(|| url.clone()),
                url,
                duration: f(e, "duration"),
            });
        }
    }

    let mut formats: Vec<FormatInfo> = Vec::new();
    if let Some(arr) = root.get("formats").and_then(|x| x.as_array()) {
        for fm in arr {
            let fid = match s(fm, "format_id") {
                Some(v) => v,
                None => continue,
            };
            formats.push(FormatInfo {
                format_id: fid,
                ext: s(fm, "ext").unwrap_or_else(|| "?".into()),
                resolution: s(fm, "resolution").or_else(|| s(fm, "format_note")),
                height: i(fm, "height"),
                vcodec: s(fm, "vcodec"),
                acodec: s(fm, "acodec"),
                filesize: i(fm, "filesize").or_else(|| i(fm, "filesize_approx")),
                tbr: f(fm, "tbr"),
                note: s(fm, "format_note"),
            });
        }
    }
    // 高清在前，方便前端直接展示
    formats.sort_by(|a, b| b.height.unwrap_or(0).cmp(&a.height.unwrap_or(0)));

    VideoInfo {
        id: s(root, "id").unwrap_or_default(),
        title: s(root, "title")
            .or_else(|| s(root, "playlist_title"))
            .unwrap_or_else(|| s(root, "webpage_url").unwrap_or_else(|| "未命名".into())),
        uploader: s(root, "uploader").or_else(|| s(root, "channel")),
        duration: f(root, "duration"),
        thumbnail: s(root, "thumbnail"),
        webpage_url: s(root, "webpage_url")
            .or_else(|| s(root, "original_url"))
            .unwrap_or_default(),
        extractor: s(root, "extractor").unwrap_or_else(|| s(root, "extractor_key").unwrap_or_default()),
        is_playlist,
        entry_count: entries.len(),
        entries,
        formats,
    }
}

/// 解析一个链接（-J；对播放列表用 --flat-playlist 只取骨架，快）
pub fn probe(url: &str, cookies: Option<&str>, proxy: Option<&str>) -> Result<VideoInfo, String> {
    let kernel = locate()?;
    let mut args: Vec<String> = vec![
        "--ignore-config".into(),
        "-J".into(),
        "--flat-playlist".into(),
        "--no-warnings".into(),
        "--socket-timeout".into(),
        "20".into(),
    ];
    args.extend(js_runtime_args());
    if let Some(c) = cookies {
        args.push("--cookies-from-browser".into());
        args.push(c.to_string());
    }
    if let Some(p) = proxy {
        args.push("--proxy".into());
        args.push(p.to_string());
    }
    args.push(url.to_string());

    let out = Command::new(&kernel.path)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("执行 yt-dlp 失败: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        let msg = extract_error(&stderr).unwrap_or_else(|| {
            let tail: String = stderr.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
            if tail.trim().is_empty() {
                "解析失败：yt-dlp 没有返回任何内容".to_string()
            } else {
                tail.trim().to_string()
            }
        });
        friendly_error(&msg)
    })?;
    if value.is_null() {
        return Err(friendly_error(
            &extract_error(&stderr).unwrap_or_else(|| "解析失败：链接无内容".into()),
        ));
    }
    Ok(parse_info(&value))
}

/// 把 yt-dlp 的英文报错翻译成「下一步该点什么」的提示。
///
/// 这些坑是 YouTube 现在的常态（SABR 流 + 机器人校验），原样抛给用户
/// 等于没说，所以这里补一句可执行的建议。
pub fn friendly_error(raw: &str) -> String {
    let low = raw.to_lowercase();
    if low.contains("sign in to confirm") || low.contains("not a bot") {
        return format!(
            "{raw}\n\n提示：YouTube 需要 Cookies 才认人。到「设置 → 从浏览器取 Cookie」选一个本机已登录 YouTube 的浏览器（chromium / chrome / firefox / edge / brave），再解析一次。"
        );
    }
    if low.contains("no video formats found") {
        return format!(
            "{raw}\n\n提示：YouTube 的解签名需要 JavaScript 运行时。装好 node / deno / bun 任一（本机已有 node 就不用动），或用 LYCODL_JS_RUNTIME=node:/path/to/node 显式指定；同时确认已在设置里选了浏览器 Cookie。"
        );
    }
    if low.contains("requested format is not available") {
        return format!(
            "{raw}\n\n提示：该链接没有你选的画质/格式。先在「解析」里看可用格式，或用自定义格式模式挑一个存在的 format_id。"
        );
    }
    if low.contains("the web client only works when logged-in") || low.contains("logged-in") {
        return format!(
            "{raw}\n\n提示：这个站点要求登录。到「设置 → 从浏览器取 Cookie」选一个已登录该站的浏览器。"
        );
    }
    if low.contains("private video") || low.contains("members-only") || low.contains("login required") {
        return format!("{raw}\n\n提示：这是私有/会员内容，需要对应账号的 Cookie。");
    }
    if low.contains("http error 403") || low.contains("forbidden") {
        return format!("{raw}\n\n提示：被站点限流或防盗链拦了。可换 Cookie、挂代理（设置页「代理」），或先更新内核试试。");
    }
    if low.contains("unsupported url") {
        return format!("{raw}\n\n提示：yt-dlp 不认识这个链接。确认它是视频页面地址而不是分享短链/首页。");
    }
    raw.to_string()
}

fn extract_error(stderr: &str) -> Option<String> {
    let mut best: Option<String> = None;
    for line in stderr.lines() {
        let t = line.trim();
        if let Some(idx) = t.find("ERROR:") {
            best = Some(t[idx + 6..].trim().to_string());
        }
    }
    best.filter(|s| !s.is_empty())
}

// ── 下载 ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DownloadOpts {
    pub url: String,
    pub mode: String,
    pub quality: String,
    pub format_id: Option<String>,
    pub playlist: bool,
    pub output_dir: PathBuf,
    pub cookies_from_browser: Option<String>,
    pub proxy: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Title(String),
    Prog {
        downloaded: i64,
        total: Option<i64>,
        speed: Option<f64>,
        eta: Option<i64>,
    },
    File(String),
    Log(String),
}

fn fmt_selector(mode: &str, quality: &str, format_id: Option<&str>) -> Vec<String> {
    match mode {
        "audio" => vec![
            "-x".into(),
            "--audio-format".into(),
            "mp3".into(),
            "--audio-quality".into(),
            "0".into(),
        ],
        "custom" => match format_id {
            Some(f) if !f.trim().is_empty() => vec!["-f".into(), f.trim().to_string()],
            _ => vec!["-f".into(), "bv*+ba/b".into()],
        },
        _ => {
            let q = quality.trim();
            if q.is_empty() || q == "best" {
                vec!["-f".into(), "bv*+ba/b".into()]
            } else {
                vec![
                    "-f".into(),
                    format!("bv*[height<={q}]+ba/b[height<={q}]/b"),
                ]
            }
        }
    }
}

pub fn build_args(o: &DownloadOpts) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--ignore-config".into(),
        "--newline".into(),
        "--no-color".into(),
        "--progress".into(),
        "--progress-template".into(),
        "download:LYCOPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.speed)s|%(progress.eta)s".into(),
        "--print".into(),
        "before_dl:LYCOTITLE %(title)s".into(),
        "--print".into(),
        "after_move:LYCOFILE %(filepath)s".into(),
        "--no-warnings".into(),
        "--concurrent-fragments".into(),
        "4".into(),
        "--socket-timeout".into(),
        "30".into(),
        "-P".into(),
        o.output_dir.display().to_string(),
        "-o".into(),
        "%(title)s [%(id)s].%(ext)s".into(),
    ];

    // YouTube 的解签名离不开 JS 运行时，缺了它只会拿到 zero formats
    a.extend(js_runtime_args());

    a.extend(fmt_selector(&o.mode, &o.quality, o.format_id.as_deref()));

    if o.mode != "audio" {
        a.push("--merge-output-format".into());
        a.push("mp4".into());
        a.push("--remux-video".into());
        a.push("mp4".into());
    }
    a.push(if o.playlist {
        "--yes-playlist".into()
    } else {
        "--no-playlist".into()
    });
    if let Some(c) = o.cookies_from_browser.as_deref() {
        a.push("--cookies-from-browser".into());
        a.push(c.to_string());
    }
    if let Some(p) = o.proxy.as_deref() {
        a.push("--proxy".into());
        a.push(p.to_string());
    }
    a.push(o.url.clone());
    a
}

fn num_or_none(v: &str) -> Option<f64> {
    let t = v.trim();
    if t.is_empty() || t == "NA" || t == "None" {
        None
    } else {
        t.parse::<f64>().ok()
    }
}

/// 跑一次下载，逐行把事件喂给 sink。返回最终文件路径（拿不到则空串）。
///
/// cancel 置位后杀子进程并返回 Err("已取消")。
pub fn download_blocking<F>(o: &DownloadOpts, cancel: Arc<AtomicBool>, mut sink: F) -> Result<String, String>
where
    F: FnMut(Event),
{
    let kernel = locate()?;
    let args = build_args(o);
    sink(Event::Log(format!(
        "命令：{} {}",
        kernel.path.display(),
        args.iter().filter(|a| !a.starts_with("LYCO")).cloned().collect::<Vec<_>>().join(" ")
    )));

    let mut child = Command::new(&kernel.path)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 yt-dlp 失败: {e}"))?;

    let stderr = child.stderr.take().expect("stderr piped");
    let errbuf: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let eb = errbuf.clone();
    let err_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            if let Ok(mut buf) = eb.lock() {
                buf.push(line.trim_end().to_string());
                if buf.len() > 200 {
                    buf.remove(0);
                }
            }
            line.clear();
        }
    });

    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut final_file = String::new();
    let mut cancelled = false;

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            let _ = child.kill();
            break;
        }
        let t = line.trim_end();
        if let Some(rest) = t.strip_prefix("LYCOPROG|") {
            let parts: Vec<&str> = rest.split('|').collect();
            if parts.len() >= 4 {
                let downloaded = num_or_none(parts[0]).unwrap_or(0.0) as i64;
                let total = num_or_none(parts[1]).map(|v| v as i64);
                let speed = num_or_none(parts[2]);
                let eta = num_or_none(parts[3]).map(|v| v as i64);
                sink(Event::Prog {
                    downloaded,
                    total,
                    speed,
                    eta,
                });
            }
        } else if let Some(rest) = t.strip_prefix("LYCOTITLE ") {
            let title = rest.trim();
            if !title.is_empty() && title != "NA" {
                sink(Event::Title(title.to_string()));
            }
        } else if let Some(rest) = t.strip_prefix("LYCOFILE ") {
            let p = rest.trim();
            if !p.is_empty() && p != "NA" {
                final_file = p.to_string();
                sink(Event::File(final_file.clone()));
            }
        } else if !t.is_empty() {
            sink(Event::Log(t.to_string()));
        }
    }

    let status = child.wait().map_err(|e| format!("等待 yt-dlp 结束失败: {e}"))?;
    let _ = err_thread.join();
    let errors = errbuf.lock().map(|b| b.clone()).unwrap_or_default();

    if cancelled || cancel.load(Ordering::Relaxed) {
        return Err("已取消".into());
    }
    if !status.success() {
        let joined = errors
            .iter()
            .filter(|l| l.contains("ERROR"))
            .cloned()
            .collect::<Vec<_>>();
        let msg = if joined.is_empty() {
            errors.last().cloned().unwrap_or_default()
        } else {
            joined.join("\n")
        };
        let msg = msg.trim();
        let raw = if msg.is_empty() {
            format!("yt-dlp 退出码 {}", status.code().unwrap_or(-1))
        } else {
            msg.to_string()
        };
        return Err(friendly_error(&raw));
    }
    Ok(final_file)
}

// ── 热更新 ──────────────────────────────────────────────────

pub fn asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        if cfg!(target_env = "musl") {
            "yt-dlp_musllinux_aarch64"
        } else {
            "yt-dlp_linux_aarch64"
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_env = "musl") {
            "yt-dlp_musllinux"
        } else {
            "yt-dlp_linux"
        }
    } else {
        "yt-dlp"
    }
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("lycodl/0.1 (+https://github.com/yt-dlp/yt-dlp)")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
}

/// 问上游最新版本号（不需要下载）
pub async fn latest_version() -> Result<String, String> {
    let resp = http()?
        .get(GITHUB_LATEST)
        .send()
        .await
        .map_err(|e| format!("访问 GitHub 失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub 返回 {}", resp.status()));
    }
    let rel: Release = resp.json().await.map_err(|e| format!("解析 release 失败: {e}"))?;
    Ok(rel.tag_name)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn expected_sha(sums: &str, asset: &str) -> Option<String> {
    for line in sums.lines() {
        let mut it = line.split_whitespace();
        let hex = it.next()?;
        let name = it.next().unwrap_or("");
        if name.trim_start_matches('*') == asset {
            return Some(hex.to_lowercase());
        }
    }
    None
}

/// 下载上游最新内核，校验 SHA256，原子替换到托管路径。
/// 返回 (旧版本, 新版本, 新路径)。
pub async fn update() -> Result<(String, String, PathBuf), String> {
    let current = locate().map(|k| k.version).unwrap_or_else(|_| "无".into());
    let client = http()?;
    // 先探一次上游版本，顺带确认网络与 GitHub API 可达
    let _latest = latest_version().await?;
    let asset = asset_name();

    // 校验和清单（拿不到就拒绝安装 —— 宁可失败也不装未经校验的二进制）
    let sums_url = format!("{GITHUB_DL_PREFIX}/SHA2-256SUMS");
    let sums = client
        .get(&sums_url)
        .send()
        .await
        .map_err(|e| format!("下载校验和失败: {e}"))?
        .text()
        .await
        .map_err(|e| format!("读取校验和失败: {e}"))?;
    let expected = expected_sha(&sums, asset)
        .ok_or_else(|| format!("上游校验和清单里没有 {asset}"))?;

    let url = format!("{GITHUB_DL_PREFIX}/{asset}");
    let bytes = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载内核失败: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取内核数据失败: {e}"))?;
    if bytes.len() < 1024 {
        return Err(format!("下载到的内核只有 {} 字节，疑似失败", bytes.len()));
    }
    let got = sha256_hex(&bytes);
    if got != expected {
        return Err(format!(
            "校验失败：期望 {expected}，实际 {got}（已丢弃，未覆盖现有内核）"
        ));
    }

    let dest = crate::paths::managed_ytdlp();
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 {} 失败: {e}", dir.display()))?;
    }
    let tmp = dest.with_extension("download");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置可执行位失败: {e}"))?;
    }
    // 先验证新二进制真能跑，再原子替换（避免装上一个坏内核）
    let new_version = version_of(&tmp)
        .map_err(|e| format!("新内核无法执行，已放弃替换: {e}"))?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("替换内核失败: {e}"))?;

    Ok((current, new_version, dest))
}

/// 是否存在由 lycodl 托管的可执行内核
pub fn managed_exists() -> bool {
    crate::paths::managed_ytdlp().is_file()
}

/// 一次性抓完整输出（内部用，给 probe/命令用）
#[allow(dead_code)]
pub fn capture(exe: &Path, args: &[String]) -> Result<(String, String), String> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动失败: {e}"))?;
    let mut so = String::new();
    let mut se = String::new();
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_string(&mut so);
    }
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut se);
    }
    let _ = child.wait();
    Ok((so, se))
}
