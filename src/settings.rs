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
    if s.cookies_from_browser.as_deref().is_some_and(|v| v.trim().is_empty()) {
        s.cookies_from_browser = None;
    }
    if s.proxy.as_deref().is_some_and(|v| v.trim().is_empty()) {
        s.proxy = None;
    }
    if s.impersonate.as_deref().is_some_and(|v| v.trim().is_empty()) {
        s.impersonate = None;
    }
    if s
        .flaresolverr_endpoint
        .as_deref()
        .is_some_and(|v| v.trim().is_empty() || v.trim() == "none")
    {
        s.flaresolverr_endpoint = None;
    }
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
