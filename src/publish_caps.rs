// ── 发布前只读可用性 / 账号已发布筛选 ──────────────────────────────
// 目的：把「不可用的 API / 登录态」和「我的账号里已经发过的文件」在真正进入
// driver 之前挡掉。所有检查只读，不上传、不发帖、不创建平台作品。
//
// 平台细节放在同目录的 Node 探针里（LYCODL_PUBLISH_CAPABILITIES 可覆盖），
// Rust 侧只负责合并本机 ledger、规范化结果和生成任务日志。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

use crate::settings::Settings;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishCapability {
    pub dest: String,
    pub available: bool,
    pub published: bool,
    /// ready / unavailable / published
    pub reason: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishProbe {
    pub file: String,
    pub size: u64,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub caps: Vec<PublishCapability>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub checked_at: u64,
}

impl PublishProbe {
    /// 一键发布真正会交到 driver 的目的地：可用、且不是「已发布」。
    pub fn selectable(&self, force: bool) -> Vec<String> {
        let order = crate::publish::DESTS;
        let mut out = Vec::new();
        for dest in order {
            let cap = self.caps.iter().find(|c| c.dest == dest);
            let Some(cap) = cap else { continue };
            if !cap.available { continue }
            if !force && cap.published { continue }
            out.push(dest.to_string());
        }
        out
    }

    pub fn skipped(&self, force: bool) -> Vec<PublishCapability> {
        self.caps
            .iter()
            .filter(|c| !c.available || (!force && c.published))
            .cloned()
            .collect()
    }

    pub fn as_value(&self, force: bool) -> Value {
        json!({
            "file": self.file,
            "size": self.size,
            "targets": self.targets,
            "selectable": self.selectable(force),
            "skipped": self.skipped(force),
            "caps": self.caps,
            "errors": self.errors,
            "checkedAt": self.checked_at,
            "force": force,
        })
    }

    pub fn log_lines(&self, force: bool) -> Vec<String> {
        let mut lines = vec![format!(
            "发布前筛选：{} 个可用未发布 / {} 个已勾选",
            self.selectable(force).len(),
            self.targets.len()
        )];
        for c in &self.caps {
            let mark = if !c.available {
                "不可用（跳过）"
            } else if !force && c.published {
                "账号已发布（跳过）"
            } else {
                "可用"
            };
            let detail = if c.detail.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", c.detail)
            };
            lines.push(format!("筛选 {}: {}{}", c.dest, mark, detail));
        }
        for e in &self.errors {
            lines.push(format!("筛选告警: {e}"));
        }
        lines
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeProbeResult {
    #[serde(default)]
    caps: Vec<PublishCapability>,
    #[serde(default)]
    checked_at: u64,
    #[serde(default)]
    errors: Vec<String>,
}

fn probe_script() -> PathBuf {
    if let Ok(p) = std::env::var("LYCODL_PUBLISH_CAPABILITIES") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    crate::paths::data_dir().join("publish-capabilities.mjs")
}

fn canonical_file(raw: &str) -> Result<(String, String, u64), String> {
    let p = PathBuf::from(raw.trim());
    if !p.is_file() {
        return Err(format!("文件不存在: {}", raw.trim()));
    }
    let p = std::fs::canonicalize(&p).unwrap_or(p);
    let file = p.display().to_string();
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    Ok((file, name, size))
}

/// 调只读 Node 探针，然后合并本机 ledger 的「已成功发布」记录。
/// 探针挂掉也要返回结构，让 UI 明确显示 unavailable，而不是悄悄真发。
pub async fn probe(file_raw: &str, targets: Vec<String>, s: &Settings) -> Result<PublishProbe, String> {
    let (file, file_name, size) = canonical_file(file_raw)?;
    let wanted = crate::publish::normalize_targets(targets);
    let script = probe_script();
    let mut errors: Vec<String> = Vec::new();
    let mut node_caps_checked_at = now_ms();
    let node_caps = if script.is_file() {
        let payload = json!({
            "file": file,
            "fileName": file_name,
            "size": size,
            "targets": wanted,
            "cdpUrl": s.publish_cdp_url,
            "quarkDir": s.publish_quark_dir,
            "platformCommand": s.publish_platform_command,
        });
        let cmd = format!("node {}", shell_quote(&script.display().to_string()));
        let child = match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => Some(c),
            Err(e) => {
                errors.push(format!("能力探针启动失败: {e}"));
                None
            }
        };
        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut exit_code: Option<i32> = None;
        if let Some(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(payload.to_string().as_bytes()).await;
                drop(stdin);
            }
            match tokio::time::timeout(std::time::Duration::from_secs(25), child.wait_with_output()).await {
                Ok(Ok(o)) => {
                    exit_code = o.status.code();
                    stdout = o.stdout;
                    stderr = o.stderr;
                }
                Ok(Err(e)) => errors.push(format!("能力探针执行失败: {e}")),
                Err(_) => {
                    errors.push("能力探针超时".into());
                }
            }
        }
        let text = String::from_utf8_lossy(&stdout);
        let mut caps = Vec::new();
        let mut node_checked_at = 0u64;
        for line in text.lines().rev() {
            if let Ok(v) = serde_json::from_str::<NodeProbeResult>(line.trim()) {
                caps = v.caps;
                errors.extend(v.errors);
                node_checked_at = v.checked_at;
                break;
            }
        }
        if caps.is_empty() {
            let err = String::from_utf8_lossy(&stderr).trim().to_string();
            errors.push(format!(
                "能力探针无结果: {}",
                if err.is_empty() { format!("exit {:?}", exit_code) } else { err.chars().take(250).collect() }
            ));
        }
        if node_checked_at == 0 {
            node_checked_at = now_ms();
        }
        node_caps_checked_at = node_checked_at;
        caps
    } else {
        errors.push(format!("能力探针不存在: {}", script.display()));
        Vec::new()
    };

    // 本机 ledger 是「文件×目的地已成功外发」的可靠去重源；
    // 夸克账号网盘列表由 Node 探针只读给出；两者合并。
    let ledger_done = crate::publish::published_dests(&file, size);
    let mut caps: Vec<PublishCapability> = wanted
        .iter()
        .map(|dest| {
            let mut c = node_caps
                .iter()
                .find(|x| &x.dest == dest)
                .cloned()
                .unwrap_or(PublishCapability {
                    dest: dest.clone(),
                    available: false,
                    published: false,
                    reason: "unavailable".into(),
                    detail: "能力探针没有返回该目的地".into(),
                    source: "fallback".into(),
                });
            if ledger_done.iter().any(|x| x == dest) {
                c.published = true;
                c.source = if c.source.is_empty() { "ledger".into() } else { format!("{}+ledger", c.source) };
                c.detail = if c.detail.trim().is_empty() {
                    "任务台账显示已成功发布".into()
                } else {
                    format!("{}；任务台账显示已成功发布", c.detail)
                };
            }
            if !c.available {
                c.reason = "unavailable".into();
            } else if c.published {
                c.reason = "published".into();
            } else {
                c.reason = "ready".into();
            }
            c
        })
        .collect();
    caps.sort_by_key(|c| {
        crate::publish::DESTS
            .iter()
            .position(|d| *d == c.dest)
            .unwrap_or(usize::MAX)
    });
    Ok(PublishProbe {
        file,
        size,
        targets: wanted,
        caps,
        errors,
        checked_at: node_caps_checked_at,
    })
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_selectable_skips_unavailable_and_published() {
        let p = PublishProbe {
            file: "/tmp/a.mp4".into(),
            size: 10,
            targets: vec!["quark".into(), "x".into(), "youtube".into()],
            caps: vec![
                PublishCapability { dest: "quark".into(), available: true, published: false, reason: "ready".into(), detail: String::new(), source: "test".into() },
                PublishCapability { dest: "x".into(), available: false, published: false, reason: "unavailable".into(), detail: String::new(), source: "test".into() },
                PublishCapability { dest: "youtube".into(), available: true, published: true, reason: "published".into(), detail: String::new(), source: "test".into() },
            ],
            errors: vec![],
            checked_at: 1,
        };
        assert_eq!(p.selectable(false), vec!["quark".to_string()]);
        assert_eq!(p.selectable(true), vec!["quark".to_string(), "youtube".to_string()]);
        assert_eq!(p.skipped(false).len(), 2);
        assert!(p.log_lines(false).iter().any(|l| l.contains("账号已发布")));
    }

    #[test]
    fn shell_quote_handles_single_quote() {
        assert_eq!(shell_quote("/tmp/a'b.mjs"), "'/tmp/a'\\''b.mjs'");
    }
}
