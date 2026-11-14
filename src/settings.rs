//! 持久化配置（~/.config/lycodl/settings.json）

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 下载输出目录
    pub output_dir: String,
    /// 从浏览器取 Cookie：chrome / firefox / edge / brave / chromium（None = 不取）
    pub cookies_from_browser: Option<String>,
    /// HTTP 代理，如 http://127.0.0.1:7890
    pub proxy: Option<String>,
    /// 同时下载数
    pub concurrency: usize,
    /// 浏览器指纹伪装目标（如 chrome / safari；None = 不开）
    ///
    /// 这是兜底手段：只有站点了 Cloudflare 之类的前置防护、裸请求被 403 时才需要。
    /// 默认关着 —— 它对绝大多数站点没有任何提升，反而多一层伪装开销。
    pub impersonate: Option<String>,
    /// 用本机浏览器代开页面（CDP 桥）：导出 cookie / 嗅探媒体地址
    ///
    /// 这是 extractor 全挂时的兜底 —— 浏览器永远是对的，它加载的页面里就有播放器
    /// 真实在用的地址。默认关着：每次要多起一个 chromium，十几秒。
    pub browser_bridge: bool,
    /// FlareSolverr 地址，如 http://127.0.0.1:8191（None = 不用）
    ///
    /// 只用来「代取页面 HTML」，不让它给 cookie —— cf_clearance 绑定 IP+TLS 指纹，
    /// 搬给 yt-dlp 一定 403。
    pub flaresolverr_endpoint: Option<String>,
    /// 下载完成后自动发布（默认开）
    ///
    /// 关掉它就退回「纯下载器」；开着时每个成品文件下载完成后按下面的目的地
    /// 各发一次，结果写进该任务的日志，并落一条 ledger 防重复。
    pub auto_publish: bool,
    /// 发布 driver 命令（用 sh -c 执行；空 = 用 data_dir/publish-driver.mjs）
    pub publish_driver: String,
    /// 发布目的地顺序：quark / bilibili / douyin / xhs / kuaishou / youtube / x
    pub publish_targets: Vec<String>,
    /// 夸克网盘里的目标目录
    pub publish_quark_dir: String,
    /// 分享链接与提取码（X 推文里带上，也给平台提交器当素材）
    pub publish_share_url: Option<String>,
    pub publish_share_passcode: Option<String>,
    /// CDP 调试浏览器地址（X / 平台发布要用已登录的标签页）
    pub publish_cdp_url: String,
    /// 平台提交命令（可选：替代 MultiPost 页面里的钩子函数）
    pub publish_platform_command: Option<String>,
    /// 演练模式：只打印「将要发到哪」不真正外发
    pub publish_dry_run: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: crate::paths::default_output_dir().display().to_string(),
            cookies_from_browser: None,
            proxy: None,
            concurrency: 2,
            impersonate: None,
            browser_bridge: false,
            flaresolverr_endpoint: None,
            auto_publish: true,
            publish_driver: String::new(),
            publish_targets: crate::publish::default_targets(),
            publish_quark_dir: crate::publish::DEFAULT_QUARK_DIR.to_string(),
            publish_share_url: None,
            publish_share_passcode: None,
            publish_cdp_url: crate::publish::DEFAULT_CDP_URL.to_string(),
            publish_platform_command: None,
            publish_dry_run: false,
        }
    }
}

pub fn config_path() -> PathBuf {
    crate::paths::config_dir().join("settings.json")
}

/// 读配置；文件缺失或损坏都退回默认值（绝不因配置崩掉整个程序）
pub fn load() -> Settings {
    let mut s = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
        .unwrap_or_default();
    if let Ok(o) = std::env::var("LYCODL_OUTPUT_DIR") {
        if !o.trim().is_empty() {
            s.output_dir = o;
        }
    }
    if s.output_dir.trim().is_empty() {
        s.output_dir = crate::paths::default_output_dir().display().to_string();
    }
    if s.concurrency == 0 {
        s.concurrency = 1;
    }
    if s.cookies_from_browser
        .as_deref()
        .is_some_and(|v| v.trim().is_empty())
    {
        s.cookies_from_browser = None;
    }
    if s.proxy.as_deref().is_some_and(|v| v.trim().is_empty()) {
        s.proxy = None;
    }
    if s.impersonate
        .as_deref()
        .is_some_and(|v| v.trim().is_empty())
    {
        s.impersonate = None;
    }
    if s.flaresolverr_endpoint
        .as_deref()
        .is_some_and(|v| v.trim().is_empty() || v.trim() == "none")
    {
        s.flaresolverr_endpoint = None;
    }
    // 发布相关的空串归一：前端清空输入框就等于「关掉这一项」
    if s.publish_share_url
        .as_deref()
        .is_some_and(|v| v.trim().is_empty())
    {
        s.publish_share_url = None;
    }
    if s.publish_share_passcode
        .as_deref()
        .is_some_and(|v| v.trim().is_empty())
    {
        s.publish_share_passcode = None;
    }
    if s.publish_platform_command
        .as_deref()
        .is_some_and(|v| v.trim().is_empty())
    {
        s.publish_platform_command = None;
    }
    if s.publish_cdp_url.trim().is_empty() {
        s.publish_cdp_url = crate::publish::DEFAULT_CDP_URL.to_string();
    }
    if s.publish_quark_dir.trim().is_empty() {
        s.publish_quark_dir = crate::publish::DEFAULT_QUARK_DIR.to_string();
    }
    s.publish_targets = crate::publish::normalize_targets(s.publish_targets.clone());
    s
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let body = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("写配置失败: {e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("提交配置失败: {e}"))?;
    Ok(())
}

/// 确保输出目录存在
pub fn ensure_output_dir(s: &Settings) -> Result<PathBuf, String> {
    let p = PathBuf::from(s.output_dir.trim());
    if p.as_os_str().is_empty() {
        return Err("输出目录为空".into());
    }
    std::fs::create_dir_all(&p).map_err(|e| format!("创建输出目录 {} 失败: {e}", p.display()))?;
    Ok(p)
}
