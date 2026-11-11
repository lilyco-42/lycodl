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
    match js_runtime_best() {
        Some((name, path)) => vec!["--js-runtimes".into(), format!("{name}:{}", path.display())],
        None => Vec::new(),
    }
}

/// 本机最合适的 JS 运行时。caps 自检与 --js-runtimes 参数共用这一份判断，
/// 避免「自检说有、实际没用上」这种自相矛盾。
pub fn js_runtime_best() -> Option<(&'static str, PathBuf)> {
    if let Ok(v) = std::env::var("LYCODL_JS_RUNTIME") {
        let v = v.trim();
        if !v.is_empty() {
            if let Some((name, path)) = v.split_once(':') {
                let name: &'static str = match name {
                    "node" => "node",
                    "deno" => "deno",
                    "bun" => "bun",
                    _ => "node",
                };
                if !path.trim().is_empty() {
                    return Some((name, PathBuf::from(path.trim())));
                }
            }
            return Some(("node", PathBuf::from(v)));
        }
    }
    js_runtime_candidates().into_iter().next()
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
///
/// 全平台支持的难点不在「能不能解析」，而在「换一个站的坑长什么样」。
/// 所以这里不是一次性硬扛，而是按代价从低到高排一串策略逐级尝试：
/// 默认配置 → 自动取本机浏览器 Cookie → 指纹伪装 → 伪装+Cookie → 站点专属后手。
/// 哪一级成功就把那一级记进策略记忆，下次同站直接走同一条路。
pub fn probe(o: &ProbeOpts) -> Result<ProbeOutcome, String> {
    let site = crate::sites::match_url(&o.url);
    let plans = build_plans(o, site.as_ref().and_then(|s| crate::sites::strategy_for(&s.key)));
    let total = plans.len();
    let mut attempts: Vec<ProbeAttempt> = Vec::new();
    let mut last_error: Option<String> = None;

    // 直接问 yt-dlp 的那几种很便宜，最多试 4 次；
    // 起浏览器 / 找 FlareSolverr 一次十几秒，只在开启时额外给一次机会。
    let heavy = plans.iter().filter(|p| p.kind != PlanKind::Ytdlp).count();
    let max_attempts = 4 + heavy;

    for (i, plan) in plans.iter().enumerate() {
        if i > 0 && attempts.len() >= max_attempts {
            break;
        }
        match probe_once(o, plan) {
            Ok(hit) => {
                attempts.push(ProbeAttempt {
                    name: plan.name.to_string(),
                    label: plan.label.to_string(),
                    ok: true,
                    error: None,
                });
                if let Some(s) = &site {
                    crate::sites::remember(&s.key, plan.name);
                }
                return Ok(ProbeOutcome {
                    info: hit.info,
                    direct: hit.direct,
                    strategy: ProbeStrategy {
                        applied: plan.name.to_string(),
                        label: plan.label.to_string(),
                        attempts,
                    },
                });
            }
            Err(raw) => {
                let friendly = friendly_error(&raw);
                attempts.push(ProbeAttempt {
                    name: plan.name.to_string(),
                    label: plan.label.to_string(),
                    ok: false,
                    error: Some(first_line(&friendly)),
                });
                let terminal = terminal_error(&raw);
                last_error = Some(friendly);
                if terminal {
                    break;
                }
            }
        }
    }

    let mut msg = last_error.unwrap_or_else(|| "解析失败：所有策略都试过了".to_string());
    if attempts.len() > 1 {
        let tried: Vec<String> = attempts
            .iter()
            .map(|a| format!("{}（{}）", a.label, if a.ok { "成功" } else { "失败" }))
            .collect();
        msg.push_str(&format!(
            "\n\n已尝试 {total} 种方式：{}。最后一次的报错见上。",
            tried.join(" → ")
        ));
    }
    Err(msg)
}

/// 解析策略的一次尝试
/// 一次尝试「怎么试」
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanKind {
    /// 直接问 yt-dlp
    Ytdlp,
    /// 起本机浏览器代开页面：能嗅到直链就直链，否则拿它挣到的 cookie 再问 yt-dlp
    Browser,
    /// 让 FlareSolverr 代取页面，从 HTML 里挖直链
    Flare,
}

#[derive(Debug, Clone)]
pub struct ProbePlan {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: PlanKind,
    pub cookies: Option<String>,
    /// 浏览器桥导出的 Netscape cookie 文件（与 cookies 二选一）
    pub cookies_file: Option<PathBuf>,
    /// 与 cookie 配套的 UA（浏览器就是这么发请求的，换一个反而露馅）
    pub user_agent: Option<String>,
    pub impersonate: Option<String>,
    pub extra: Vec<String>,
}

fn build_plans(o: &ProbeOpts, remembered: Option<String>) -> Vec<ProbePlan> {
    let site_imp = o.impersonate.clone().filter(|s| !s.trim().is_empty());
    let auto_imp = crate::caps::default_impersonate_target();
    let auto_cookie = crate::caps::auto_cookie_browser();
    let is_youtube = o.url.contains("youtube.com") || o.url.contains("youtu.be");

    let mut plans: Vec<ProbePlan> = Vec::new();
    plans.push(ProbePlan {
        name: "default",
        label: "默认配置",
        kind: PlanKind::Ytdlp,
        cookies: o.cookies_from_browser.clone(),
        cookies_file: None,
        user_agent: None,
        impersonate: site_imp.clone(),
        extra: Vec::new(),
    });
    if o.cookies_from_browser.is_none() {
        if let Some(b) = auto_cookie.clone() {
            plans.push(ProbePlan {
                name: "cookies",
                label: "自动取本机浏览器 Cookie",
                kind: PlanKind::Ytdlp,
                cookies: Some(b),
                cookies_file: None,
                user_agent: None,
                impersonate: site_imp.clone(),
                extra: Vec::new(),
            });
        }
    }
    if site_imp.is_none() {
        if let Some(t) = auto_imp.clone() {
            plans.push(ProbePlan {
                name: "impersonate",
                label: "浏览器指纹伪装",
                kind: PlanKind::Ytdlp,
                cookies: o.cookies_from_browser.clone(),
                cookies_file: None,
                user_agent: None,
                impersonate: Some(t.clone()),
                extra: Vec::new(),
            });
            if o.cookies_from_browser.is_none() {
                if let Some(b) = auto_cookie.clone() {
                    plans.push(ProbePlan {
                        name: "impersonate+cookies",
                        label: "指纹伪装 + 浏览器 Cookie",
                        kind: PlanKind::Ytdlp,
                        cookies: Some(b),
                        cookies_file: None,
                        user_agent: None,
                        impersonate: Some(t),
                        extra: Vec::new(),
                    });
                }
            }
        }
    }
    if is_youtube {
        plans.push(ProbePlan {
            name: "youtube-client",
            label: "YouTube 备用播放客户端",
            kind: PlanKind::Ytdlp,
            cookies: o.cookies_from_browser.clone().or(auto_cookie),
            cookies_file: None,
            user_agent: None,
            impersonate: site_imp,
            extra: vec![
                "--extractor-args".into(),
                "youtube:player_client=web_safari".into(),
            ],
        });
    }

    // 下面两条是「重武器」：extractor 全线失守时才值得花十几秒
    if o.browser_bridge && crate::browser::chromium_path().is_some() {
        plans.push(ProbePlan {
            name: "browser",
            label: "本机浏览器代开页面",
            kind: PlanKind::Browser,
            cookies: None,
            cookies_file: None,
            user_agent: None,
            impersonate: None,
            extra: Vec::new(),
        });
    }
    if o.flaresolverr.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
        plans.push(ProbePlan {
            name: "flaresolverr",
            label: "FlareSolverr 代取页面",
            kind: PlanKind::Flare,
            cookies: None,
            cookies_file: None,
            user_agent: None,
            impersonate: None,
            extra: Vec::new(),
        });
    }

    // 记住的策略提到最前面：这条站以前就是靠它通的
    if let Some(key) = remembered {
        if let Some(pos) = plans.iter().position(|p| p.name == key) {
            let p = plans.remove(pos);
            plans.insert(0, p);
        }
    }
    plans
}

struct ProbeHit {
    info: VideoInfo,
    direct: Option<DirectMedia>,
}

fn probe_once(o: &ProbeOpts, plan: &ProbePlan) -> Result<ProbeHit, String> {
    match plan.kind {
        PlanKind::Ytdlp => Ok(ProbeHit {
            info: probe_ytdlp(o, plan)?,
            direct: None,
        }),
        PlanKind::Browser => {
            // 起一个真浏览器把页面打开：过了反爬就顺手把 cookie 和媒体请求都收走
            let out = crate::browser::bridge(&o.url, 12_000)?;
            if let Some(hit) = crate::browser::pick_media(&out.media) {
                let dm = DirectMedia {
                    url: hit.url.clone(),
                    // 直链多半校验 Referer，原页面地址就是浏览器当时带的那个
                    referer: Some(o.url.clone()),
                    user_agent: Some(out.user_agent.clone()),
                    source: "browser".into(),
                };
                return Ok(ProbeHit {
                    info: direct_info(o, &hit, &out.title, "browser"),
                    direct: Some(dm),
                });
            }
            // 没嗅到媒体，但浏览器可能已经替我们解掉了反爬：拿它的 cookie 再问一次 yt-dlp
            let note = if out.challenge_passed {
                format!(
                    "浏览器侧：页面已打开（标题「{}」），抓到 {} 个媒体请求，但都像是装饰或预览片段",
                    out.title,
                    out.media.len()
                )
            } else {
                format!(
                    "浏览器侧：页面停在反爬挑战上（标题「{}」），本机 Chromium 没能放行——这类 CF 站点请改用 FlareSolverr",
                    out.title
                )
            };
            let cf = out.cookies_file.clone().ok_or_else(|| {
                format!("{note}；也没拿到可用的 Cookie")
            })?;
            let mut p2 = plan.clone();
            p2.cookies_file = Some(cf);
            p2.user_agent = Some(out.user_agent.clone());
            probe_ytdlp(o, &p2).map(|info| ProbeHit { info, direct: None }).map_err(|e| {
                // yt-dlp 的报错只说它自己看到什么；浏览器实际看到了什么，只有这里能说清楚
                format!("{e}

{note}")
            })
        }
        PlanKind::Flare => {
            let ep = o
                .flaresolverr
                .clone()
                .ok_or_else(|| "未配置 FlareSolverr 地址".to_string())?;
            let page = crate::browser::flaresolverr_media(&ep, &o.url)?;
            let dm = DirectMedia {
                url: page.hit.url.clone(),
                referer: Some(o.url.clone()),
                user_agent: None,
                source: "flaresolverr".into(),
            };
            Ok(ProbeHit {
                info: direct_info(o, &page.hit, &page.title, "flaresolverr"),
                direct: Some(dm),
            })
        }
    }
}

/// 浏览器 / FlareSolverr 拿到的是「一条真实在播的媒体地址」，
/// 没有 extractor 给的元数据，这里拼一个够前端展示的 VideoInfo。
fn direct_info(o: &ProbeOpts, hit: &crate::browser::MediaHit, page_title: &str, source: &str) -> VideoInfo {
    let title = {
        let t = page_title.trim();
        if t.is_empty() {
            o.url.clone()
        } else {
            t.to_string()
        }
    };
    let digest = sha256_hex(hit.url.as_bytes());
    let id = digest.chars().take(11).collect::<String>();
    let ext = guess_ext(&hit.url, &hit.mime);
    VideoInfo {
        id,
        title,
        uploader: None,
        duration: None,
        thumbnail: None,
        webpage_url: o.url.clone(),
        extractor: source.to_string(),
        is_playlist: false,
        entry_count: 0,
        entries: Vec::new(),
        formats: vec![FormatInfo {
            format_id: "direct".into(),
            ext,
            resolution: None,
            height: None,
            vcodec: None,
            acodec: None,
            filesize: if hit.size > 0 { Some(hit.size as i64) } else { None },
            tbr: None,
            note: Some(match source {
                "flaresolverr" => "FlareSolverr 代取页面挖到的直链".to_string(),
                _ => "浏览器里真实在播的直链".to_string(),
            }),
        }],
    }
}

fn guess_ext(url: &str, mime: &str) -> String {
    let low = url.to_lowercase();
    if low.contains(".m3u8") || mime.contains("mpegurl") || mime.contains("dash+xml") || low.contains(".mpd") {
        // HLS/DASH 最终都会被合成 mp4
        return "mp4".into();
    }
    for e in ["mp4", "webm", "mkv", "flv", "m4a", "mp3", "aac", "ogg"] {
        if low.contains(&format!(".{e}")) {
            return e.into();
        }
    }
    "mp4".into()
}

fn probe_ytdlp(o: &ProbeOpts, plan: &ProbePlan) -> Result<VideoInfo, String> {
    let mut args: Vec<String> = vec![
        "--ignore-config".into(),
        "-J".into(),
        "--flat-playlist".into(),
        "--no-warnings".into(),
        "--socket-timeout".into(),
        "20".into(),
    ];
    args.extend(js_runtime_args());
    if let Some(c) = plan.cookies.as_deref() {
        args.push("--cookies-from-browser".into());
        args.push(c.to_string());
    }
    if let Some(f) = plan.cookies_file.as_deref() {
        args.push("--cookies".into());
        args.push(f.display().to_string());
    }
    if let Some(ua) = plan.user_agent.as_deref() {
        // 浏览器桥给的 cookie 是配着它自己的 UA 发的，换 UA 会被判成另一个客户端
        args.push("--user-agent".into());
        args.push(ua.to_string());
    }
    if let Some(p) = o.proxy.as_deref() {
        args.push("--proxy".into());
        args.push(p.to_string());
    }
    if let Some(imp) = plan.impersonate.as_deref() {
        args.push("--impersonate".into());
        args.push(imp.to_string());
    }
    args.extend(plan.extra.iter().cloned());
    args.push(o.url.clone());

    let kernel = locate()?;
    let out = Command::new(&kernel.path)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("执行 yt-dlp 失败: {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        let msg = extract_error(&stderr).unwrap_or_else(|| {
            let tail: String = stderr
                .chars()
                .rev()
                .take(600)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if tail.trim().is_empty() {
                "解析失败：yt-dlp 没有返回任何内容".to_string()
            } else {
                tail.trim().to_string()
            }
        });
        msg
    })?;
    if value.is_null() {
        return Err(extract_error(&stderr).unwrap_or_else(|| "解析失败：链接无内容".into()));
    }
    let info = parse_info(&value);
    // 「解析成功但一个格式都没有」对下载器等于失败：上游 extractor 认了页面却没找到流。
    // 这种时候必须继续降级（浏览器/FlareSolverr 还有机会看到真实在播的那条流），
    // 不能当成成功把降级链堵死。
    if info.formats.is_empty() && !info.is_playlist {
        return Err(format!(
            "No video formats found!（{} 认了页面但没解析出可下载的流）",
            info.extractor
        ));
    }
    Ok(info)
}

/// 这条错误再换策略也没用（链接本身的问题，不是环境的问题）
fn terminal_error(raw: &str) -> bool {
    let low = raw.to_lowercase();
    [
        "unsupported url",
        "this video is unavailable",
        "video is offline",
        "no video could be found",
        "can't find any video",
        "http error 404",
        "not found",
        "video unavailable",
        "has been removed",
    ]
    .iter()
    .any(|k| low.contains(k))
}

/// 策略 key → 中文名。
///
/// 存进 strategies.json 的是 key（稳定、可被 build_plans 比对），
/// 但接口把它回给前端时要给人看，所以这里做一次翻译。
pub fn strategy_label(name: &str) -> &'static str {
    match name {
        "default" => "默认配置",
        "cookies" => "自动取本机浏览器 Cookie",
        "impersonate" => "浏览器指纹伪装",
        "impersonate+cookies" => "指纹伪装 + 浏览器 Cookie",
        "youtube-client" => "YouTube 备用播放客户端",
        "browser" => "本机浏览器代开页面",
        "flaresolverr" => "FlareSolverr 代取页面",
        _ => "自定义策略",
    }
}

fn first_line(s: &str) -> String {
    let l = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if l.chars().count() > 240 {
        format!("{}…", l.chars().take(240).collect::<String>())
    } else {
        l.to_string()
    }
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
            "{raw}\n\n提示：上游没给出可下载的视频流。三个常见原因：①YouTube 的解签名需要 JavaScript 运行时 —— 装 node / deno / bun 任一（或用 LYCODL_JS_RUNTIME=node:/path/to/node 指定）；②站点要求浏览器 Cookie（设置页「从浏览器取 Cookie」）；③这个页面本身是图集/预览图，没有正片。"
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
        return format!(
            "{raw}\n\n提示：yt-dlp 不认识这个链接。确认它是视频页面地址而不是分享短链/首页；到「平台」页搜一下站点名，看内核有没有对应提取器（没有就是真不支持）。"
        );
    }
    // ── 各站特有 / 常见的下一批坑 ────────────────────────────────

    if low.contains("fresh cookies") || low.contains("cookies (not necessarily logged in)") {
        return format!(
            "{raw}\n\n提示：这个站只认「新鲜 Cookie」，不需要登录状态。到「设置 → 从浏览器取 Cookie」选一个最近打开过该站的浏览器（多数情况 Chromium 就行）。"
        );
    }
    if low.contains("account authentication is required") {
        return format!(
            "{raw}\n\n提示：站点要求账号身份。选一个已登录该站的浏览器 Cookie；没有账号的话这条链接拿不到。"
        );
    }
    if low.contains("cannot parse data") {
        return format!(
            "{raw}\n\n提示：站点返回了登录墙而不是视频页。换成已登录该站的浏览器 Cookie 再试；Facebook 这类站点对未登录请求一律这样回。"
        );
    }
    if low.contains("geo restriction") || low.contains("not available in your country") || low.contains("geo-restricted") {
        return format!(
            "{raw}\n\n提示：地区限制。到「设置」填代理（如 http://127.0.0.1:7890）后再解析一次。"
        );
    }
    if low.contains("unable to extract") {
        return format!(
            "{raw}\n\n提示：站点页面结构变了，内核的解析规则还没跟上。到「设置 → 内核」点「检查更新」拉最新版 yt-dlp（热更新，不用重装 lycodl）。"
        );
    }
    if low.contains("timed out") || low.contains("timeout") || low.contains("connection") {
        return format!(
            "{raw}\n\n提示：网络连不上或太慢。挂个代理（设置页「代理」），或者在能直连的网络下重试。"
        );
    }
    if low.contains("impersonate target") {
        return format!(
            "{raw}\n\n提示：这个内核没有该指纹伪装目标。到「平台 → 运行环境自检」看可用列表，或在设置里关掉指纹伪装。"
        );
    }
    if low.contains("no video formats found") && low.contains("storyboard") {
        return format!(
            "{raw}\n\n提示：上游对这个视频只下发了预览图（storyboard），拿不到正片。换一个视频试试，不是本项目的问题。"
        );
    }
    if low.contains("this video is unavailable") || low.contains("video unavailable") {
        return format!(
            "{raw}\n\n提示：视频已被删除/私有/地区屏蔽。先确认在浏览器里能正常播放，再考虑 Cookie 或代理。"
        );
    }
    if low.contains("video is offline") || low.contains("is offline") {
        return format!("{raw}\n\n提示：直播已结束或未开播。直播要么在直播中带 Cookie 抓，要么等录播生成后再下。");
    }
    if low.contains("requested format is not available") {
        return format!(
            "{raw}\n\n提示：这个链接没有你选的画质。先解析看格式列表，或改用「自定义格式」填一个存在的 format_id。"
        );
    }
    if low.contains("no video could be found") {
        return format!("{raw}\n\n提示：这条内容里没有视频（纯文字/纯图片）。换一条带视频的链接。");
    }
    if low.contains("can't find any video") {
        return format!(
            "{raw}\n\n提示：上游没给出可下载的视频流（常见于需要会员或风控的页面）。换 Cookie 或换链接。"
        );
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

/// 解析请求：cookies / proxy 来自设置，impersonate 也来自设置
#[derive(Debug, Clone, Default)]
pub struct ProbeOpts {
    pub url: String,
    pub cookies_from_browser: Option<String>,
    pub proxy: Option<String>,
    pub impersonate: Option<String>,
    /// 允许起本机浏览器代开页面（慢，但 extractor 全挂时是唯一的路）
    pub browser_bridge: bool,
    /// FlareSolverr 地址（可选外部服务）
    pub flaresolverr: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    pub info: VideoInfo,
    pub strategy: ProbeStrategy,
    /// 浏览器/FlareSolverr 嗅到的直链：extractor 全挂时的兜底下载入口
    pub direct: Option<DirectMedia>,
}

/// 页面里真实在用的媒体地址（不经过 extractor）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectMedia {
    pub url: String,
    /// 有些 CDN 校验 Referer，下载时要原样带上
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    /// browser / flaresolverr
    pub source: String,
}

/// 一次策略尝试的记录（前端想看「试过哪几种、各自为什么失败」）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeAttempt {
    pub name: String,
    pub label: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeStrategy {
    /// 最终生效的策略名（default / cookies / impersonate / impersonate+cookies / youtube-client）
    pub applied: String,
    pub label: String,
    pub attempts: Vec<ProbeAttempt>,
}

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
    /// 浏览器指纹伪装（curl_cffi）。只对 Cloudflare 类站点有用，按站点设或按需降级
    pub impersonate: Option<String>,
    /// extractor 全挂时，浏览器/FlareSolverr 嗅到的直链：直接拿它下载
    pub direct: Option<DirectMedia>,
    /// 直链没有标题（extractor 没解析过），用解析阶段看到的页面标题当文件名
    pub direct_title: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// 一项开始前的标题；播放列表里带「第几项 / 共几项」
    Title {
        index: Option<u64>,
        count: Option<u64>,
        title: String,
    },
    Prog {
        downloaded: i64,
        total: Option<i64>,
        speed: Option<f64>,
        eta: Option<i64>,
        /// 进度行所属的播放列表序号（单视频为 None）
        index: Option<u64>,
    },
    File(String),
    /// 非致命问题（播放列表里某一项失败被跳过）：整体仍成功，但要让人知道漏了什么
    Warn(String),
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
        // 末尾两个字段是播放列表序号：单视频时 yt-dlp 会打印 NA，解析成 None
        "download:LYCOPROG|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.speed)s|%(progress.eta)s|%(info.playlist_index)s".into(),
        "--print".into(),
        "before_dl:LYCOTITLE|%(playlist_index)s|%(n_entries)s|%(title)s".into(),
        "--print".into(),
        // before_dl 阶段就有 %(filename)s：不含扩展名的完整输出路径。
        // 取消时要按这个基名去清掉 *.part / *.ytdl 半成品（SIGKILL 来不及让 yt-dlp 自己清）
        "before_dl:LYCOBASE %(filename)s".into(),
        "--print".into(),
        "after_move:LYCOFILE %(filepath)s".into(),
        "--no-warnings".into(),
        "--concurrent-fragments".into(),
        "4".into(),
        "--socket-timeout".into(),
        "30".into(),
        // 网络抖动不该让整条下载白跑
        "--retries".into(),
        "10".into(),
        "--fragment-retries".into(),
        "10".into(),
        "--retry-sleep".into(),
        "2".into(),
        "-P".into(),
        o.output_dir.display().to_string(),
        "-o".into(),
        "%(title)s [%(id)s].%(ext)s".into(),
    ];

    // YouTube 的解签名离不开 JS 运行时，缺了它只会拿到 zero formats
    a.extend(js_runtime_args());

    // 合并视频+音频全靠 ffmpeg：显式指路，PATH 里没有时也不会静默失败
    if let Some(dir) = crate::caps::ffmpeg_dir() {
        a.push("--ffmpeg-location".into());
        a.push(dir);
    }

    // 直链没有格式表可挑：它就是唯一的那条流，再套 -f 反而会挑空。
    // 但「只要音频」是用户意图，-x 仍然要加。
    match (&o.direct, o.mode.as_str()) {
        (None, _) => a.extend(fmt_selector(&o.mode, &o.quality, o.format_id.as_deref())),
        (Some(_), "audio") => a.extend(fmt_selector("audio", &o.quality, None)),
        (Some(_), _) => {}
    }

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
    if let Some(imp) = o.impersonate.as_deref() {
        let imp = imp.trim();
        if !imp.is_empty() {
            a.push("--impersonate".into());
            a.push(imp.to_string());
        }
    }

    if let Some(d) = &o.direct {
        // CDN 大多校验 Referer 与 UA：不原样带上就是 403
        if let Some(r) = d.referer.as_deref() {
            a.push("--referer".into());
            a.push(r.to_string());
        }
        if let Some(ua) = d.user_agent.as_deref() {
            a.push("--user-agent".into());
            a.push(ua.to_string());
        }
        // 直链没有 [id]，靠标题去重会撞名，加个固定后缀区分
        if let Some(t) = o.direct_title.as_deref() {
            let safe = sanitize_name(t);
            if !safe.is_empty() {
                a.push("-o".into());
                a.push(format!("{safe} [direct].%(ext)s"));
            }
        }
        a.push(d.url.clone());
        return a;
    }

    a.push(o.url.clone());
    a
}

/// 文件系统里不能出现的字符统一换掉（直链拿到的标题常常带 / : ? 之类）
fn sanitize_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => out.push(' '),
            _ => out.push(c),
        }
    }
    let t = out.trim().trim_matches('.').trim();
    t.chars().take(120).collect()
}

fn num_or_none(v: &str) -> Option<f64> {
    let t = v.trim();
    if t.is_empty() || t == "NA" || t == "None" {
        None
    } else {
        t.parse::<f64>().ok()
    }
}

/// 杀掉整棵 yt-dlp 进程组（含 PyInstaller 再 fork 出来的真身）。
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    unsafe {
        // 进程组 id == setsid 后的 pid，负数表示「发给整个组」
        libc::kill(-pid, libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// 取消后清掉这次下载留下的半成品（*.part / *.ytdl / *.temp / 分段 *.part-FragN）。
/// 失败（非取消）不清，留着让 yt-dlp 下次续传。
fn cleanup_partial(base: &str) -> usize {
    let pb = std::path::Path::new(base);
    let dir = match pb.parent() {
        Some(d) => d.to_path_buf(),
        None => return 0,
    };
    let name = match pb.file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => return 0,
    };
    // %(filename)s 带扩展名（.webm / .f397.mp4 等），去掉最后一截再当前缀：
    // 这样视频轨的 xxx.f397.mp4.part 和音频轨的 xxx.m4a.part 都能一并点到。
    let stem = match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.clone(),
    };
    let prefix = format!("{stem}.");
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with(&prefix) {
                continue;
            }
            let is_partial = name.contains(".part")
                || name.ends_with(".ytdl")
                || name.ends_with(".temp")
                || name.ends_with(".frag");
            if is_partial && std::fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
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

    let mut cmd = Command::new(&kernel.path);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // yt-dlp 官方 linux 包是 PyInstaller 单文件：启动后父进程会再 fork 一个真身。
    // 只 kill 父进程会留下孤儿继续下载、并且还握着 stdout 管道 —— 读线程永远等不到 EOF。
    // 所以把它放进独立进程组，取消时按组杀。
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("启动 yt-dlp 失败: {e}"))?;

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

    // 读 stdout 必须丢到独立线程：播放列表换项时 yt-dlp 会安静几十秒（在解析下一项），
    // 主线程如果直接阻塞在 read_line 上，就检查不到 cancel 标志 —— 实测取消能拖到两分钟。
    let stdout = child.stdout.take().expect("stdout piped");
    let (ltx, lrx) = std::sync::mpsc::channel::<String>();
    let reader_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if ltx.send(line.clone()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut final_file = String::new();
    let mut completed: Vec<String> = Vec::new();
    let mut current_base: Option<String> = None;
    let mut cancelled = false;

    loop {
        let raw = match lrx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(l) => l,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    kill_tree(&mut child);
                    break;
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            kill_tree(&mut child);
            break;
        }
        let t = raw.trim_end();
        if let Some(rest) = t.strip_prefix("LYCOPROG|") {
            let parts: Vec<&str> = rest.split('|').collect();
            if parts.len() >= 4 {
                let downloaded = num_or_none(parts[0]).unwrap_or(0.0) as i64;
                let total = num_or_none(parts[1]).map(|v| v as i64);
                let speed = num_or_none(parts[2]);
                let eta = num_or_none(parts[3]).map(|v| v as i64);
                let index = parts.get(4).and_then(|v| num_or_none(v)).map(|v| v as u64);
                sink(Event::Prog {
                    downloaded,
                    total,
                    speed,
                    eta,
                    index,
                });
            }
        } else if let Some(rest) = t.strip_prefix("LYCOTITLE|") {
            // LYCOPROG 的字段顺序是 fixed，所以这里也按 | 切
            let f: Vec<&str> = rest.splitn(3, '|').collect();
            if f.len() == 3 {
                let index = num_or_none(f[0]).map(|v| v as u64);
                let count = num_or_none(f[1]).map(|v| v as u64);
                let title = f[2].trim();
                if !title.is_empty() && title != "NA" {
                    sink(Event::Title {
                        index,
                        count,
                        title: title.to_string(),
                    });
                }
            }
        } else if let Some(rest) = t.strip_prefix("LYCOBASE ") {
            let b = rest.trim();
            if !b.is_empty() && b != "NA" {
                current_base = Some(b.to_string());
            }
        } else if let Some(rest) = t.strip_prefix("LYCOFILE ") {
            let p = rest.trim();
            if !p.is_empty() && p != "NA" {
                final_file = p.to_string();
                completed.push(p.to_string());
                sink(Event::File(final_file.clone()));
            }
        } else if !t.is_empty() {
            sink(Event::Log(t.to_string()));
        }
    }

    let status = child.wait().map_err(|e| format!("等待 yt-dlp 结束失败: {e}"))?;
    let _ = err_thread.join();
    // 取消的场合已经 kill_tree 过：此时管道写端可能还被祖孙进程短暂握着，
    // 硬等读线程会让「取消」看起来像卡住 —— 直接放手，让它自己收尾。
    if cancelled {
        drop(reader_thread);
        if let Some(b) = &current_base {
            let n = cleanup_partial(b);
            if n > 0 {
                sink(Event::Log(format!("已取消，清理 {n} 个未完成的临时文件")));
            }
        }
        return Err("已取消".into());
    }
    let _ = reader_thread.join();
    let errors = errbuf.lock().map(|b| b.clone()).unwrap_or_default();

    if cancel.load(Ordering::Relaxed) {
        return Err("已取消".into());
    }
    if !status.success() {
        let joined = errors
            .iter()
            .filter(|l| l.contains("ERROR"))
            .cloned()
            .collect::<Vec<_>>();
        // 播放列表 + --ignore-errors：yt-dlp 会因为「有项失败」返回非零，但只要真有成品落盘，
        // 这批就是部分成功。此时不该把整个任务判死，而要把失败项报出来让人知道。
        if o.playlist && !completed.is_empty() {
            let skipped: Vec<String> = joined
                .iter()
                .map(|l| friendly_error(l))
                .collect::<Vec<_>>();
            if skipped.is_empty() {
                sink(Event::Warn(format!(
                    "有部分条目未能下载，成功 {} 项",
                    completed.len()
                )));
            } else {
                for w in &skipped {
                    sink(Event::Warn(w.clone()));
                }
                sink(Event::Warn(format!(
                    "成功 {} 项，其余条目已跳过",
                    completed.len()
                )));
            }
            sink(Event::Log(format!("完成：{final_file}")));
            return Ok(final_file);
        }
        // 失败时把 yt-dlp 的原始 stderr 尾部写进日志：界面上的 error 是翻译过的中文提示，
        // 排查时还是得看原文（任务面板的「日志」按钮就是为这个加的）。
        for line in errors.iter().rev().take(20).rev() {
            sink(Event::Log(line.clone()));
        }
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
    // 播放列表 + --ignore-errors 时 yt-dlp 即便有项失败也返回 0：
    // 不能让「跳过 3 项」悄无声息地算作完全成功。
    if o.playlist {
        let skipped: Vec<String> = errors
            .iter()
            .filter(|l| l.contains("ERROR"))
            .map(|l| friendly_error(l))
            .collect();
        for w in skipped.iter().take(20) {
            sink(Event::Warn(w.clone()));
        }
        if !skipped.is_empty() {
            sink(Event::Warn(format!(
                "共 {} 个条目失败已跳过，成功 {} 项",
                skipped.len(),
                completed.len()
            )));
        }
    }
    if !final_file.is_empty() {
        sink(Event::Log(format!("完成：{final_file}")));
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
    let latest = latest_version().await?;
    // 已经是最新就别白拉 41 MB —— 除非用户明确要求强制重装
    let force = std::env::var("LYCODL_KERNEL_FORCE").map(|v| v == "1").unwrap_or(false);
    if !force && current == latest && managed_exists() {
        let path = crate::paths::managed_ytdlp();
        return Ok((current.clone(), current, path));
    }
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
