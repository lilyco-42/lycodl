//! lilyco 命令面：CLI / TUI / MCP 三个前端共用同一套命令定义。
//!
//! Web 控制台走自己的 REST 接口（见 api.rs），但它展示的命令清单也来自这里，
//! 所以「命令行能做什么」与「AI 能做什么」永远是同一份真相。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use lilyco::prelude::*;
use serde_json::{json, Value};

use crate::settings;
use crate::ytdlp::{self, DownloadOpts, Event};

// ── 解析 ───────────────────────────────────────────────────────

#[derive(App)]
#[app(name = "probe", about = "解析链接信息，列出可用格式（不下载）", run = "run_probe")]
pub struct ProbeCmd {
    /// 视频或播放列表链接
    pub url: String,
}

fn run_probe(app: &ProbeCmd, ctx: &Context) -> Result<Value, AppError> {
    let s = settings::load();
    ctx.log(LogLevel::Info, "正在解析…");
    let info = ytdlp::probe(
        &app.url,
        s.cookies_from_browser.as_deref(),
        s.proxy.as_deref(),
    )
    .map_err(AppError::Runtime)?;
    let out = json!(info);
    ctx.done(out.clone(), 0);
    Ok(out)
}

#[derive(App)]
#[app(name = "formats", about = "只列格式表（人类可读）", run = "run_formats")]
pub struct FormatsCmd {
    /// 视频链接
    pub url: String,
}

fn run_formats(app: &FormatsCmd, ctx: &Context) -> Result<Value, AppError> {
    let s = settings::load();
    let info = ytdlp::probe(
        &app.url,
        s.cookies_from_browser.as_deref(),
        s.proxy.as_deref(),
    )
    .map_err(AppError::Runtime)?;
    ctx.log(LogLevel::Info, format!("{}（{}）", info.title, info.extractor));
    for f in &info.formats {
        let size = f
            .filesize
            .map(|b| format!("{:.1} MB", b as f64 / 1_048_576.0))
            .unwrap_or_else(|| "-".to_string());
        ctx.log(
            LogLevel::Info,
            format!(
                "{:<16} {:<6} {:<12} {:<10} {}",
                f.format_id,
                f.ext,
                f.resolution.clone().unwrap_or_else(|| "-".into()),
                size,
                f.note.clone().unwrap_or_default()
            ),
        );
    }
    let out = json!({ "title": info.title, "formats": info.formats });
    ctx.done(out.clone(), 0);
    Ok(out)
}

// ── 下载 ───────────────────────────────────────────────────────

#[derive(App)]
#[app(
    name = "download",
    about = "下载视频 / 音频（进度实时上报）",
    run = "run_download",
    safety = "confirm"
)]
pub struct DownloadCmd {
    /// 视频或播放列表链接
    pub url: String,
    /// 模式：video | audio | custom
    #[arg(default = "video")]
    pub mode: String,
    /// 画质上限：best | 2160 | 1440 | 1080 | 720 | 480
    #[arg(default = "best")]
    pub quality: String,
    /// 自定义格式 ID（mode=custom 时必填，可用 formats 命令查）
    pub format_id: Option<String>,
    /// 下载整个播放列表
    #[arg(default = false)]
    pub playlist: bool,
}

fn run_download(app: &DownloadCmd, ctx: &Context) -> Result<Value, AppError> {
    if !matches!(app.mode.as_str(), "video" | "audio" | "custom") {
        return Err(AppError::InvalidArg(format!("未知模式: {}", app.mode)));
    }
    if app.mode == "custom" && app.format_id.as_deref().unwrap_or("").trim().is_empty() {
        return Err(AppError::InvalidArg("custom 模式必须给 --format-id".into()));
    }
    let s = settings::load();
    let out_dir = settings::ensure_output_dir(&s).map_err(AppError::Runtime)?;
    let opts = DownloadOpts {
        url: app.url.clone(),
        mode: app.mode.clone(),
        quality: app.quality.clone(),
        format_id: app.format_id.clone(),
        playlist: app.playlist,
        output_dir: out_dir.clone(),
        cookies_from_browser: s.cookies_from_browser.clone(),
        proxy: s.proxy.clone(),
    };

    ctx.log(LogLevel::Info, format!("输出目录: {}", out_dir.display()));
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_sink = cancel.clone();
    let file = ytdlp::download_blocking(&opts, cancel, |ev| {
        let _ = &cancel_for_sink;
        match ev {
            Event::Title { title, .. } => ctx.log(LogLevel::Info, format!("标题: {title}")),
            Event::Prog {
                downloaded,
                total,
                speed,
                eta,
                ..
            } => {
                let pct = match total {
                    Some(t) if t > 0 => downloaded as f64 / t as f64 * 100.0,
                    _ => 0.0,
                };
                let sp = speed.unwrap_or(0.0) / 1_048_576.0;
                let msg = format!(
                    "{:>5.1}%  {:.1} MB/s  ETA {}",
                    pct,
                    sp,
                    eta.map(|e| format!("{e}s")).unwrap_or_else(|| "-".into())
                );
                ctx.tick(downloaded.max(0) as u64, total.map(|t| t.max(0) as u64), msg);
            }
            Event::File(p) => ctx.log(LogLevel::Info, format!("完成: {p}")),
            Event::Warn(w) => ctx.log(LogLevel::Warn, format!("跳过: {w}")),
            Event::Log(l) => ctx.log(LogLevel::Debug, l),
        }
    })
    .map_err(AppError::Runtime)?;

    let out = json!({ "ok": true, "file": file, "outputDir": out_dir.display().to_string() });
    ctx.done(out.clone(), 0);
    Ok(out)
}

// ── 内核（yt-dlp）热更新 ────────────────────────────────────────

#[derive(App)]
#[app(name = "kernel", about = "查看当前 yt-dlp 内核位置与版本", run = "run_kernel")]
pub struct KernelCmd {}

fn run_kernel(_app: &KernelCmd, ctx: &Context) -> Result<Value, AppError> {
    let (info, out) = match ytdlp::locate() {
        Ok(k) => {
            let out = json!({
                "path": k.path.display().to_string(),
                "version": k.version,
                "source": k.source,
                "managed": ytdlp::managed_exists(),
            });
            ctx.log(
                LogLevel::Info,
                format!("yt-dlp {} @ {}", out["version"], out["path"]),
            );
            (None, out)
        }
        Err(e) => {
            ctx.log(LogLevel::Warn, format!("未找到 yt-dlp: {e}"));
            (Some(e.clone()), json!({ "error": e }))
        }
    };
    let _ = info;
    ctx.done(out.clone(), 0);
    Ok(out)
}

#[derive(App)]
#[app(
    name = "kernel-update",
    about = "从 GitHub 拉取最新 yt-dlp 覆盖本地托管副本（热更新）",
    run = "run_kernel_update",
    safety = "confirm"
)]
pub struct KernelUpdateCmd {}

fn run_kernel_update(_app: &KernelUpdateCmd, ctx: &Context) -> Result<Value, AppError> {
    ctx.log(LogLevel::Info, "查询 GitHub 最新版本…");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Runtime(format!("runtime: {e}")))?;
    let res = rt.block_on(ytdlp::update());
    match res {
        Ok((from, to, path)) => {
            ctx.log(
                LogLevel::Info,
                format!("已更新 {} → {} @ {}", from, to, path.display()),
            );
            let out = json!({
                "ok": true,
                "from": from,
                "alreadyLatest": from == to,
                "to": to,
                "path": path.display().to_string(),
            });
            ctx.done(out.clone(), 0);
            Ok(out)
        }
        Err(e) => Err(AppError::Runtime(e)),
    }
}

// ── 配置 ───────────────────────────────────────────────────────

#[derive(App)]
#[app(name = "settings", about = "查看当前配置", run = "run_settings")]
pub struct SettingsShowCmd {}

fn run_settings(_app: &SettingsShowCmd, ctx: &Context) -> Result<Value, AppError> {
    let s = settings::load();
    let out = serde_json::to_value(&s).unwrap_or_else(|_| json!({}));
    ctx.log(
        LogLevel::Info,
        format!("配置文件: {}", settings::config_path().display()),
    );
    ctx.done(out.clone(), 0);
    Ok(out)
}

#[derive(App)]
#[app(
    name = "settings-set",
    about = "修改配置（只改动传入的字段）",
    run = "run_settings_set",
    safety = "confirm"
)]
pub struct SettingsSetCmd {
    /// 输出目录
    pub output_dir: Option<String>,
    /// 从浏览器读 Cookie：chrome / firefox / edge / brave / chromium
    pub cookies: Option<String>,
    /// HTTP 代理
    pub proxy: Option<String>,
    /// 同时下载数
    #[arg(min = 1, max = 16)]
    pub concurrency: Option<usize>,
}

fn run_settings_set(app: &SettingsSetCmd, ctx: &Context) -> Result<Value, AppError> {
    let mut s = settings::load();
    if let Some(v) = app.output_dir.as_ref() {
        if !v.trim().is_empty() {
            s.output_dir = v.trim().to_string();
        }
    }
    if let Some(v) = app.cookies.as_ref() {
        s.cookies_from_browser = if v.trim().is_empty() || v.trim() == "none" {
            None
        } else {
            Some(v.trim().to_string())
        };
    }
    if let Some(v) = app.proxy.as_ref() {
        s.proxy = if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().to_string())
        };
    }
    if let Some(n) = app.concurrency {
        s.concurrency = n.max(1);
    }
    settings::save(&s).map_err(AppError::Runtime)?;
    settings::ensure_output_dir(&s).map_err(AppError::Runtime)?;
    ctx.log(LogLevel::Info, "配置已保存");
    let out = serde_json::to_value(&s).unwrap_or_else(|_| json!({}));
    ctx.done(out.clone(), 0);
    Ok(out)
}

// ── 产物 ───────────────────────────────────────────────────────

#[derive(App)]
#[app(name = "files", about = "列出输出目录里的成品文件", run = "run_files")]
pub struct FilesCmd {}

fn run_files(_app: &FilesCmd, ctx: &Context) -> Result<Value, AppError> {
    let s = settings::load();
    let dir = settings::ensure_output_dir(&s).map_err(AppError::Runtime)?;
    let mut items: Vec<Value> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            items.push(json!({
                "name": name,
                "size": meta.len(),
                "path": p.display().to_string(),
            }));
        }
    }
    items.sort_by(|a, b| {
        a.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });
    for it in &items {
        ctx.log(
            LogLevel::Info,
            format!(
                "{}  ({:.1} MB)",
                it["name"].as_str().unwrap_or("-"),
                it["size"].as_u64().unwrap_or(0) as f64 / 1_048_576.0
            ),
        );
    }
    let out = json!({ "dir": dir.display().to_string(), "files": items });
    ctx.done(out.clone(), 0);
    Ok(out)
}

// ── 注册表 ─────────────────────────────────────────────────────

/// 组装命令面（CLI / TUI / MCP / Web 命令清单共用）
pub fn registry() -> Registry {
    registry_with_policy(Arc::new(lilyco::prelude::DenyElevated))
}

/// 用指定安全策略**从头**装配注册表。
///
/// 必须这样做的原因：lilyco 的安全门在 `Registry::register` 时就把策略闭包
/// 包进了 handler，事后换策略只会把新门套在旧门外面，旧门仍然拒绝。
/// 所以想放行 T1 命令，策略必须在注册之前就位。
pub fn registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);
    let cmds: Vec<RegisteredCommand> = vec![
        RegisteredCommand::from_app::<ProbeCmd>(),
        RegisteredCommand::from_app::<FormatsCmd>(),
        RegisteredCommand::from_app::<DownloadCmd>(),
        RegisteredCommand::from_app::<FilesCmd>(),
        RegisteredCommand::from_app::<KernelCmd>(),
        RegisteredCommand::from_app::<KernelUpdateCmd>(),
        RegisteredCommand::from_app::<SettingsShowCmd>(),
        RegisteredCommand::from_app::<SettingsSetCmd>(),
    ];
    for c in cmds {
        if let Err(e) = reg.register(c) {
            eprintln!("lycodl: 注册命令失败: {e}");
        }
    }
    reg
}
