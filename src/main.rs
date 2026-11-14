//! lycodl —— yt-dlp 内核 + lilyco 命令面 + shadcn Web 控制台的视频下载器。
//!
//! 四种前端共用一份内核与一份配置：
//!   lycodl              裸跑 → 起本地 Web 控制台（主入口）
//!   lycodl --web        同上（显式）
//!   lycodl <子命令>     CLI 形态，脚本 / AI 友好
//!   lycodl --tui        TUI 形态
//!   lycodl --mcp        MCP 服务器形态，交给 Agent 调用
//!
//! 内核（yt-dlp）通过 kernel-update 子命令或 Web 设置页热更新，
//! 这样新站点 / 新防盗链永远不需要重新编译本项目。

mod api;
mod assets;
mod browser;
mod caps;
mod commands;
mod jobs;
mod paths;
mod plugin;
mod policy;
mod publish;
mod settings;
mod sites;
mod ytdlp;

use std::sync::Arc;

use lilyco::prelude::SafetyPolicy;

const BANNER: &str = "lycodl —— 多平台视频下载器（yt-dlp 内核 + lilyco）";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!(
            "lycodl {} ({} {})",
            env!("CARGO_PKG_VERSION"),
            option_env!("TARGET").unwrap_or(std::env::consts::OS),
            std::env::consts::ARCH
        );
        let kernel = crate::ytdlp::locate();
        match kernel {
            Ok(k) => println!("yt-dlp {} ({})", k.version, k.path.display()),
            Err(e) => println!("yt-dlp 不可用: {e}"),
        }
        return;
    }

    // --web / --gui 走本项目自己的 axum 控制台（不是 lilyco-gui 的通用表单页）
    let wants_web = args.iter().any(|a| a == "--web" || a == "--gui");
    let wants_web = wants_web
        || std::env::var("LILYCO_UI")
            .map(|v| v == "web" || v == "gui")
            .unwrap_or(false);

    if wants_web {
        api::serve();
        return;
    }

    // 裸跑（没有任何参数）→ 直接起 Web 控制台，这是产品主入口
    if args.is_empty() {
        println!("{BANNER}");
        println!("（裸跑默认启动 Web 控制台；CLI 用法：lycodl --help）");
        api::serve();
        return;
    }

    let backend = lilyco::detect_registry_backend();
    let policy: Arc<dyn SafetyPolicy> = match backend {
        lilyco::Backend::Mcp => Arc::new(policy::Agent),
        _ => Arc::new(policy::DownloadOnly),
    };
    // 策略必须同时交给注册表构造（门在 register 时就包住 handler）与分发入口。
    let registry = commands::registry_with_policy(Arc::clone(&policy));
    lilyco::run_registry_with_policy("lycodl", registry, backend, policy);
}
