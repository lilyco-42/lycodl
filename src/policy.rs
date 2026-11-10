//! 安全策略：命令分级 → 放行/拒绝。
//!
//! lycodl 的全部写操作只落在两处用户自有目录（配置目录与下载目录），
//! 不触碰系统面，因此本地交互面（CLI / TUI / 自带 Web 控制台）放行
//! T0 只读与 T1 需确认两级；T2/T3 仍拒绝 —— 那是留给更高危系统操作的档位。
//!
//! MCP（Agent 无人值守）另用 Agent 策略：只读全放行，写操作仅白名单命令放行，
//! 例如 kernel-update（替换自管内核）就不允许 Agent 静默执行。

use lilyco::prelude::{GateDecision, GateRequest, SafetyPolicy, SafetyTier};

/// 本地交互面策略：T0 / T1 放行，T2 / T3 拒绝
pub struct DownloadOnly;

impl SafetyPolicy for DownloadOnly {
    fn check(&self, req: GateRequest<'_>) -> GateDecision {
        match req.tier {
            SafetyTier::ReadOnly | SafetyTier::Confirm => GateDecision::Allow,
            other => GateDecision::Deny(format!(
                "命令 {} 分级为 {}：lycodl 只放行只读与本地写（T0/T1）",
                req.command,
                other.tag()
            )),
        }
    }
}

/// Agent 面策略：只读全放行；T1 仅白名单命令放行
pub struct Agent;

/// Agent 可以触发的写命令：跑一次下载、抓一次解析、改一次设置。
/// kernel-update 刻意不在名单里 —— 换内核二进制应由人类动手。
const AGENT_ALLOWED: &[&str] = &["download", "probe", "settings-set"];

impl SafetyPolicy for Agent {
    fn check(&self, req: GateRequest<'_>) -> GateDecision {
        match req.tier {
            SafetyTier::ReadOnly => GateDecision::Allow,
            SafetyTier::Confirm if AGENT_ALLOWED.contains(&req.command) => GateDecision::Allow,
            other => GateDecision::Deny(format!(
                "命令 {} 分级为 {}：Agent 调用面不放行（人类可用 lycodl --web 或 CLI 手动执行）",
                req.command,
                other.tag()
            )),
        }
    }
}
