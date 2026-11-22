//! 文件发布：下载完成自动发布 + 本机文件手动发布。
//!
//! 内核（Rust）负责知道「哪个文件要发」、把结果写回任务日志 / ledger，并给手动发布建任务；
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
    /// 批量发布时标记这一条属于哪个成品（普通任务为空）
    #[serde(default)]
    pub file_name: String,
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
    /// 手动发布任务 id（自动发布沿用下载任务 id；测试入口为 null）
    #[serde(default)]
    pub task_id: Option<String>,
    /// auto / local / upload / one-click / batch / test
    #[serde(default)]
    pub source: String,
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
pub fn published_dests(file: &str, size: u64) -> Vec<String> {
    already_ok(file, size)
}

/// 批量演练去重：同一 file×size×dest 演练过即跳过；失败/不可用始终允许重试。
fn already_attempted(file: &str, size: u64, dry_run: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in read_ledger() {
        if e.file != file || e.size != size {
            continue;
        }
        let counted = if dry_run {
            (e.status == "dry-run" && e.dry_run) || (e.status == "ok" && !e.dry_run)
        } else {
            e.status == "ok" && !e.dry_run
        };
        if counted && !out.iter().any(|x| x == &e.dest) {
            out.push(e.dest);
        }
    }
    out
}

fn driver_command(s: &crate::settings::Settings) -> String {
    driver_path(s).display().to_string()
}

/// Absolute driver path: settings override, else the data dir default.
pub fn driver_path(s: &crate::settings::Settings) -> std::path::PathBuf {
    let raw = s.publish_driver.trim();
    if !raw.is_empty() {
        return std::path::PathBuf::from(raw);
    }
    crate::paths::data_dir().join("publish-driver.mjs")
}

/// Adapter files live next to the driver, so a new platform is one JSON file.
pub fn adapters_dir(s: &crate::settings::Settings) -> std::path::PathBuf {
    driver_path(s)
        .parent()
        .map(|p| p.join("adapters"))
        .unwrap_or_else(|| crate::paths::data_dir().join("adapters"))
}

/// Parse every adapter JSON (invalid files are skipped, not fatal).
pub fn list_adapters(s: &crate::settings::Settings) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(adapters_dir(s)) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        if path.file_name().and_then(|x| x.to_str()) == Some("schema.json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            out.push(value);
        }
    }
    out.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });
    out
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 单次发布的覆盖项：自动发布用默认值；本机发布用页面里的选择。
#[derive(Debug, Clone)]
pub struct PublishOptions {
    /// None = 用设置里的全量目的地；Some(空) 也会归一成全量
    pub targets: Option<Vec<String>>,
    /// None = 跟随设置里的演练模式
    pub dry_run: Option<bool>,
    /// 忽略 ledger 已成功记录，按勾选目的地重发（可能重复外发）
    pub force: bool,
    pub task_id: Option<String>,
    /// auto / local / upload / one-click / batch / test
    pub source: String,
    /// 写进日志的动作名
    pub label: String,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            targets: None,
            dry_run: None,
            force: false,
            task_id: None,
            source: "auto".into(),
            label: "自动发布".into(),
        }
    }
}

fn effective_targets(s: &crate::settings::Settings, o: &PublishOptions) -> Vec<String> {
    match &o.targets {
        Some(list) => normalize_targets(list.clone()),
        None => s.publish_targets.clone(),
    }
}

fn effective_dry_run(s: &crate::settings::Settings, o: &PublishOptions) -> bool {
    o.dry_run.unwrap_or(s.publish_dry_run)
}

fn human_status(status: &str) -> &'static str {
    match status {
        "ok" => "成功",
        "dry-run" => "演练未外发",
        "unavailable" => "未就绪",
        _ => "失败",
    }
}

/// 把一条结果同时送给任务日志、ledger、实时任务快照和返回值。
#[allow(clippy::too_many_arguments)]
fn record_result<F, R>(
    mut r: PublishResult,
    file: &str,
    size: u64,
    dry_run: bool,
    file_name: &str,
    o: &PublishOptions,
    log: &mut F,
    on_result: &mut R,
    results: &mut Vec<PublishResult>,
) where
    F: FnMut(String),
    R: FnMut(PublishResult),
{
    r.at = now_ms();
    if r.file_name.trim().is_empty() {
        r.file_name = file_name.to_string();
    }
    log(format!(
        "发布 {}：{}（{}）",
        r.dest,
        human_status(&r.status),
        r.detail
    ));
    append_ledger(&LedgerEntry {
        file: file.to_string(),
        size,
        dest: r.dest.clone(),
        status: r.status.clone(),
        detail: r.detail.clone(),
        url: r.url.clone(),
        at: r.at,
        dry_run: dry_run || r.status == "dry-run",
        task_id: o.task_id.clone(),
        source: o.source.clone(),
    });
    on_result(r.clone());
    results.push(r);
}

fn driver_error(detail: impl Into<String>) -> PublishResult {
    PublishResult {
        dest: "driver".into(),
        status: "failed".into(),
        detail: detail.into(),
        url: None,
        at: 0,
        file_name: String::new(),
    }
}

/// 对一个成品文件跑一次发布。
///
/// pending 为空（全发过了）就直接返回空 Vec，不启动 driver。
/// log 回调把进度写进任务日志，让「发布到哪、成功没成功」在 UI 里可追踪。
#[cfg(test)]
#[allow(dead_code)]
pub async fn publish_one<F>(
    file: String,
    file_name: String,
    title: String,
    size: u64,
    s: crate::settings::Settings,
    log: F,
) -> Vec<PublishResult>
where
    F: FnMut(String),
{
    publish_one_with_progress(
        file,
        file_name,
        title,
        size,
        s,
        PublishOptions::default(),
        log,
        |_| {},
    )
    .await
}

/// 带覆盖项的发布（不要求实时结果回调）。
pub async fn publish_one_with<F>(
    file: String,
    file_name: String,
    title: String,
    size: u64,
    s: crate::settings::Settings,
    o: PublishOptions,
    log: F,
) -> Vec<PublishResult>
where
    F: FnMut(String),
{
    publish_one_with_progress(file, file_name, title, size, s, o, log, |_| {}).await
}

/// 完整发布入口：每收到一条目的地结果就回调一次，便于任务卡实时显示。
#[allow(clippy::too_many_arguments)]
pub async fn publish_one_with_progress<F, R>(
    file: String,
    file_name: String,
    title: String,
    size: u64,
    s: crate::settings::Settings,
    o: PublishOptions,
    mut log: F,
    mut on_result: R,
) -> Vec<PublishResult>
where
    F: FnMut(String),
    R: FnMut(PublishResult),
{
    let targets = effective_targets(&s, &o);
    let dry_run = effective_dry_run(&s, &o);
    let done = if o.force {
        Vec::new()
    } else {
        already_attempted(&file, size, dry_run)
    };
    let pending: Vec<String> = targets
        .iter()
        .filter(|d| !done.iter().any(|x| x == *d))
        .cloned()
        .collect();
    if pending.is_empty() {
        let reason = if dry_run {
            "已演练或发布过，跳过"
        } else {
            "已发布过，跳过"
        };
        log(format!("{}：{file_name} {reason}", o.label));
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
        "size": size,
        "fileName": file_name,
        "title": title,
        "pending": pending,
        "dryRun": dry_run,
        "quarkDir": s.publish_quark_dir,
        "shareUrl": s.publish_share_url,
        "sharePasscode": s.publish_share_passcode,
        "cdpUrl": s.publish_cdp_url,
        "platformCommand": s.publish_platform_command,
        "taskId": o.task_id,
        "source": o.source,
        "force": o.force,
    });

    log(format!(
        "{}：{file_name} → {}",
        o.label,
        pending.join(" / ")
    ));

    let mut results: Vec<PublishResult> = Vec::new();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let detail = format!("启动 driver 失败: {e}（{driver}）");
            log(format!("{}：{detail}", o.label));
            record_result(
                driver_error(detail),
                &file,
                size,
                dry_run,
                &file_name,
                &o,
                &mut log,
                &mut on_result,
                &mut results,
            );
            return results;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let body = payload.to_string();
        if let Err(e) = stdin.write_all(body.as_bytes()).await {
            log(format!("{}：写 driver 输入失败: {e}", o.label));
        }
        let _ = stdin.shutdown().await;
    }

    if let Some(out) = child.stdout.take() {
        let mut lines = BufReader::new(out).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let t = line.trim();
                    if t.is_empty() {
                        continue;
                    }
                    let r: PublishResult = match serde_json::from_str::<PublishResult>(t) {
                        Ok(v) => v,
                        Err(_) => {
                            log(format!("发布驱动输出：{t}"));
                            continue;
                        }
                    };
                    record_result(
                        r,
                        &file,
                        size,
                        dry_run,
                        &file_name,
                        &o,
                        &mut log,
                        &mut on_result,
                        &mut results,
                    );
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
            let detail = format!("发布驱动退出码 {}", st.code().unwrap_or(-1));
            log(detail.clone());
            record_result(
                driver_error(detail),
                &file,
                size,
                dry_run,
                &file_name,
                &o,
                &mut log,
                &mut on_result,
                &mut results,
            );
        }
        Err(e) => log(format!("发布驱动异常: {e}")),
        _ => {}
    }
    if !errbuf.trim().is_empty() {
        log(format!("发布驱动 stderr：{}", errbuf.trim()));
    }
    if results.is_empty() {
        log(format!("{}：driver 没有返回任何结果", o.label));
        record_result(
            driver_error("driver 没有返回任何结果"),
            &file,
            size,
            dry_run,
            &file_name,
            &o,
            &mut log,
            &mut on_result,
            &mut results,
        );
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
            task_id: None,
            source: "test".into(),
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
            task_id: None,
            source: "test".into(),
        });
        assert_eq!(already_ok("/tmp/a.mp4", 10), vec!["quark".to_string()]);
        assert_eq!(
            already_attempted("/tmp/a.mp4", 10, true),
            vec!["quark".to_string(), "x".to_string()]
        );
        assert_eq!(
            already_attempted("/tmp/a.mp4", 10, false),
            vec!["quark".to_string()]
        );
        assert!(already_ok("/tmp/a.mp4", 11).is_empty());
        assert_eq!(ledger_recent(5).len(), 2);
        std::env::remove_var("LYCODL_PUBLISH_LEDGER");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_options_override_targets_and_dry_run() {
        let s = crate::settings::Settings::default();
        let mut o = PublishOptions::default();
        assert_eq!(effective_targets(&s, &o), s.publish_targets);
        assert_eq!(effective_dry_run(&s, &o), s.publish_dry_run);
        o.targets = Some(vec!["X".into(), "quark".into(), "bogus".into()]);
        assert_eq!(
            effective_targets(&s, &o),
            vec!["quark".to_string(), "x".to_string()]
        );
        o.targets = Some(Vec::new());
        assert_eq!(effective_targets(&s, &o), default_targets());
        o.dry_run = Some(true);
        assert!(effective_dry_run(&s, &o));
    }
}
