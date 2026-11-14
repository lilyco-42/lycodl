//! 下载完成后的自动发布。
//!
//! 内核（Rust）只负责两件事：知道「哪个文件刚刚下载完成」，以及把结果写回任务日志 /
//! ledger。真正的平台动作交给可替换的 driver 脚本（默认 data_dir/publish-driver.mjs，
//! 一个说 JSON 行协议的小进程）：
//!
//!   stdin  ← {"file":"/abs/a.mp4","fileName":"a.mp4","title":"…","pending":["quark","x"],…}
//!   stdout → 每行一条 {"dest":"quark","status":"ok|failed|unavailable|dry-run","detail":"…"}
//!
//! 这样夸克上传、5 平台提交、X 发链接这些「依赖登录态 + 平台风控」的脏活可以单独换掉，
//! 不用重新编译 Rust；换驱动只要改配置里的「发布 driver 命令」。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// 夸克网盘里的默认目标目录
pub const DEFAULT_QUARK_DIR: &str = "/06_待整理/lycodl-短剧集";
/// CDP 调试浏览器默认地址（X / 平台发布要用已登录的标签页）
pub const DEFAULT_CDP_URL: &str = "http://127.0.0.1:9333";

/// 支持的目的地（也是固定发布顺序）
pub const DESTS: [&str; 7] = [
    "quark", "bilibili", "douyin", "xhs", "kuaishou", "youtube", "x",
];

pub fn default_targets() -> Vec<String> {
    DESTS.iter().map(|s| s.to_string()).collect()
}

/// 去重 + 丢掉不认识的项，并强制回到 DESTS 的固定顺序
/// （前端点选顺序不可控，且「夸克先传、链接后发」本身就是正确顺序）
pub fn normalize_targets(input: Vec<String>) -> Vec<String> {
    let mut want: Vec<String> = Vec::new();
    for raw in input {
        let k = raw.trim().to_ascii_lowercase();
        if DESTS.contains(&k.as_str()) && !want.iter().any(|x| x == &k) {
            want.push(k);
        }
    }
    if want.is_empty() {
        return default_targets();
    }
    DESTS
        .iter()
        .filter(|d| want.iter().any(|w| w == *d))
        .map(|d| d.to_string())
        .collect()
}

/// 一个目的地的发布结果（同时原样写进任务日志）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub dest: String,
    /// ok / failed / unavailable / dry-run
    pub status: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub at: u64,
}

impl PublishResult {
    #[allow(dead_code)]
    pub fn ok(&self) -> bool {
        self.status == "ok"
    }
}

/// ledger：一行一个「文件 × 目的地」的结论，保证重启 / 重试后不重复外发
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub file: String,
    pub size: u64,
    pub dest: String,
    pub status: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub dry_run: bool,
}

pub fn ledger_path() -> PathBuf {
    // 测试（以及想换盘存放的用户）可以单独重定向 ledger，
    // 不去动公共的 LYCODL_DATA_DIR —— 那个变量是全进程的，单元测试并行时会互相踩。
    if let Ok(p) = std::env::var("LYCODL_PUBLISH_LEDGER") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    crate::paths::data_dir().join("publish-ledger.jsonl")
}

fn read_ledger() -> Vec<LedgerEntry> {
    let text = match std::fs::read_to_string(ledger_path()) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LedgerEntry>(l).ok())
        .collect()
}

/// 最近的发布记录（新的在前）—— 给设置页「最近发布」用
pub fn ledger_recent(limit: usize) -> Vec<LedgerEntry> {
    let mut all = read_ledger();
    all.reverse();
    all.truncate(limit);
    all
}

fn append_ledger(e: &LedgerEntry) {
    let p = ledger_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(line) = serde_json::to_string(e) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// 同一个文件（路径 + 大小都一致才算同一个）已经成功发过的目的地
fn already_ok(file: &str, size: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in read_ledger() {
        if e.file == file
            && e.size == size
            && e.status == "ok"
            && !e.dry_run
            && !out.iter().any(|x| x == &e.dest)
        {
            out.push(e.dest);
        }
    }
    out
}

/// 只读查询：这个文件已经成功发布到哪些目的地
#[allow(dead_code)]
pub fn published_dests(file: &str, size: u64) -> Vec<String> {
    already_ok(file, size)
}

fn driver_command(s: &crate::settings::Settings) -> String {
    let raw = s.publish_driver.trim();
    if !raw.is_empty() {
        return raw.to_string();
    }
    crate::paths::data_dir()
        .join("publish-driver.mjs")
        .display()
        .to_string()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 对一个成品文件跑一次发布。
///
/// pending 为空（全发过了）就直接返回空 Vec，不启动 driver。
/// log 回调把进度写进任务日志，让「发布到哪、成功没成功」在 UI 里可追踪。
pub async fn publish_one<F>(
    file: String,
    file_name: String,
    title: String,
    size: u64,
    s: crate::settings::Settings,
    mut log: F,
) -> Vec<PublishResult>
where
    F: FnMut(String),
{
    let done = already_ok(&file, size);
    let pending: Vec<String> = s
        .publish_targets
        .iter()
        .filter(|d| !done.iter().any(|x| x == *d))
        .cloned()
        .collect();
    if pending.is_empty() {
        log(format!("自动发布：{file_name} 已发布过，跳过"));
        return Vec::new();
    }

    let driver = driver_command(&s);
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(&driver)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let payload = serde_json::json!({
        "file": file,
        "fileName": file_name,
        "title": title,
        "pending": pending,
        "dryRun": s.publish_dry_run,
        "quarkDir": s.publish_quark_dir,
        "shareUrl": s.publish_share_url,
        "sharePasscode": s.publish_share_passcode,
        "cdpUrl": s.publish_cdp_url,
        "platformCommand": s.publish_platform_command,
    });

    log(format!("自动发布：{file_name} → {}", pending.join(" / ")));

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log(format!("自动发布：启动 driver 失败: {e}（{driver}）"));
            return Vec::new();
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let body = payload.to_string();
        if let Err(e) = stdin.write_all(body.as_bytes()).await {
            log(format!("自动发布：写 driver 输入失败: {e}"));
        }
        let _ = stdin.shutdown().await;
    }

    let mut results: Vec<PublishResult> = Vec::new();
    if let Some(out) = child.stdout.take() {
        let mut lines = BufReader::new(out).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let t = line.trim();
                    if t.is_empty() {
                        continue;
                    }
                    let mut r: PublishResult = match serde_json::from_str::<PublishResult>(t) {
                        Ok(v) => v,
                        Err(_) => {
                            log(format!("发布驱动输出：{t}"));
                            continue;
                        }
                    };
                    r.at = now_ms();
                    let human = match r.status.as_str() {
                        "ok" => "成功",
                        "dry-run" => "演练未外发",
                        "unavailable" => "未就绪",
                        _ => "失败",
                    };
                    log(format!("发布 {}：{}（{}）", r.dest, human, r.detail));
                    append_ledger(&LedgerEntry {
                        file: file.clone(),
                        size,
                        dest: r.dest.clone(),
                        status: r.status.clone(),
                        detail: r.detail.clone(),
                        url: r.url.clone(),
                        at: r.at,
                        dry_run: s.publish_dry_run || r.status == "dry-run",
                    });
                    results.push(r);
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    let mut errbuf = String::new();
    if let Some(err) = child.stderr.take() {
        let mut lines = BufReader::new(err).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if errbuf.len() < 2000 {
                errbuf.push_str(&line);
                errbuf.push('\n');
            }
        }
    }
    match child.wait().await {
        Ok(st) if !st.success() => {
            log(format!("发布驱动退出码 {}", st.code().unwrap_or(-1)));
        }
        Err(e) => log(format!("发布驱动异常: {e}")),
        _ => {}
    }
    if !errbuf.trim().is_empty() {
        log(format!("发布驱动 stderr：{}", errbuf.trim()));
    }
    if results.is_empty() {
        log("自动发布：driver 没有返回任何结果".to_string());
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_normalize_keeps_known_and_order() {
        let got = normalize_targets(vec![
            "X".to_string(),
            "quark".to_string(),
            "bogus".to_string(),
            "quark".to_string(),
        ]);
        assert_eq!(got, vec!["quark".to_string(), "x".to_string()]);
    }

    #[test]
    fn targets_empty_falls_back_to_all() {
        assert_eq!(normalize_targets(Vec::new()), default_targets());
        assert_eq!(default_targets().len(), 7);
    }

    #[test]
    fn ledger_marks_only_real_success() {
        let dir = std::env::temp_dir().join(format!("lycodl-publish-test-{}", now_ms()));
        let _ = std::fs::create_dir_all(&dir);
        let ledger = dir.join("ledger.jsonl");
        std::env::set_var("LYCODL_PUBLISH_LEDGER", &ledger);
        append_ledger(&LedgerEntry {
            file: "/tmp/a.mp4".into(),
            size: 10,
            dest: "quark".into(),
            status: "ok".into(),
            detail: "done".into(),
            url: None,
            at: 1,
            dry_run: false,
        });
        append_ledger(&LedgerEntry {
            file: "/tmp/a.mp4".into(),
            size: 10,
            dest: "x".into(),
            status: "dry-run".into(),
            detail: "演练".into(),
            url: None,
            at: 2,
            dry_run: true,
        });
        assert_eq!(already_ok("/tmp/a.mp4", 10), vec!["quark".to_string()]);
        assert!(already_ok("/tmp/a.mp4", 11).is_empty());
        assert_eq!(ledger_recent(5).len(), 2);
        std::env::remove_var("LYCODL_PUBLISH_LEDGER");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
