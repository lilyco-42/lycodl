//! 下载任务表：排队、进度、取消、SSE 广播。
//!
//! 一个任务 = 一个 yt-dlp 子进程。阻塞式的子进程跑在 spawn_blocking 线程里，
//! 每读到一行进度就更新内存里的任务快照，并往 broadcast 频道推一份 —— 所有
//! SSE 订阅者共享同一条流，各自按 job id 过滤。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

use crate::ytdlp::{self, DirectMedia, DownloadOpts, Event};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl Status {
    pub fn is_active(self) -> bool {
        matches!(self, Status::Queued | Status::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub url: String,
    pub title: String,
    pub mode: String,
    /// 画质上限（重试时原样复用）
    pub quality: String,
    /// 自定义格式 ID（重试时原样复用）
    pub format_id: Option<String>,
    /// 是否整列表下载（重试时原样复用）
    pub playlist: bool,
    /// extractor 全挂时走浏览器/FlareSolverr 嗅到的直链（重试时原样复用）
    #[serde(default)]
    pub direct: Option<DirectMedia>,
    /// 直链没有 id，用解析时的页面标题当文件名（重试时原样复用）
    #[serde(default)]
    pub direct_title: Option<String>,
    pub status: Status,
    /// 0–100
    pub percent: f64,
    pub downloaded_bytes: i64,
    pub total_bytes: Option<i64>,
    pub speed: Option<f64>,
    pub eta: Option<i64>,
    pub output: Option<String>,
    pub file_name: Option<String>,
    pub error: Option<String>,
    /// 播放列表里当前是第几项（单视频为 null）
    pub playlist_index: Option<u64>,
    /// 播放列表总项数（单视频为 null）
    pub playlist_count: Option<u64>,
    /// 非致命告警（如播放列表里跳过了几项）：任务仍算成功，但要让人看见
    pub warnings: Vec<String>,
    /// 毫秒时间戳（前端按 >1e12 视为毫秒，两可）
    pub created_at: u64,
    pub ended_at: Option<u64>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct JobStore {
    inner: Arc<Mutex<HashMap<String, Job>>>,
    cancel: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    tx: broadcast::Sender<Job>,
    /// 正在跑的任务数（不用 Semaphore：令牌发出去了就收不回来，
    /// 那样「把并发数调小」永远不生效。自管计数 + 轮询等待才能真正收紧上限）
    running: Arc<std::sync::atomic::AtomicUsize>,
    /// 当前并发上限（可随时调整，调小后已在跑的不打断，新任务等名额）
    conc: Arc<std::sync::atomic::AtomicUsize>,
    seq: Arc<AtomicU64>,
    /// 日志尾部（每个任务最近若干行），用于失败排查
    logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

/// 名额守卫：任务结束（无论成功/失败/取消/提前返回）都把名额还回去
struct SlotGuard(std::sync::Arc<JobStore>);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.0.release_slot();
    }
}

pub struct NewJob {
    pub url: String,
    pub mode: String,
    pub quality: String,
    pub format_id: Option<String>,
    pub playlist: bool,
    /// 直链兜底：有值就不经过 extractor，直接下这条流
    pub direct: Option<DirectMedia>,
    pub direct_title: Option<String>,
    pub settings: crate::settings::Settings,
}

/// 下载成功后就地起一次「自动发布」。
///
/// 每个成品文件各跑一次 driver，ledger 保证同一文件（路径 + 大小）不重复外发。
/// 发布是后台任务：任务本身的 Done 状态先落地，用户马上能在文件列表里看到成品，
/// 发布进度随后以日志行追加（前端点「日志」就能看到每个目的地的结论）。
fn spawn_publish(
    store: &Arc<JobStore>,
    jid: &str,
    file: &str,
    s: &crate::settings::Settings,
    collected: &Arc<Mutex<Vec<String>>>,
) {
    if !s.auto_publish {
        return;
    }
    let mut files = collected.lock().map(|v| v.clone()).unwrap_or_default();
    if files.is_empty() {
        // 兜底：某些模式下 yt-dlp 没打 LYCOFILE，但下载线程报告了成品路径
        files.push(file.to_string());
    }
    let mut uniq: Vec<String> = Vec::new();
    for f in files {
        if !uniq.iter().any(|x| x == &f) {
            uniq.push(f);
        }
    }
    let job_title = store.get(jid).map(|j| j.title).unwrap_or_default();
    let st = store.clone();
    let jid = jid.to_string();
    let s2 = s.clone();
    tokio::spawn(async move {
        for f in uniq {
            let pb = PathBuf::from(&f);
            let fname = pb
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            let title = if job_title.trim().is_empty() {
                fname.clone()
            } else {
                job_title.clone()
            };
            let logger = st.clone();
            let lid = jid.clone();
            crate::publish::publish_one(f, fname, title, size, s2.clone(), move |line| {
                logger.push_log(&lid, line);
            })
            .await;
        }
    });
}

impl JobStore {
    pub fn new(concurrency: usize) -> Self {
        let (tx, _rx) = broadcast::channel(2048);
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            cancel: Arc::new(Mutex::new(HashMap::new())),
            tx,
            running: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            conc: Arc::new(std::sync::atomic::AtomicUsize::new(concurrency.max(1))),
            seq: Arc::new(AtomicU64::new(0)),
            logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Job> {
        self.tx.subscribe()
    }

    /// 调整并发上限：调大立刻能用满，调小后已在跑的任务不被打断，
    /// 但空出来的名额不会再被填上，直到活跃数降到新上限以下。
    pub fn set_concurrency(&self, n: usize) {
        self.conc.store(n.max(1), Ordering::SeqCst);
    }

    pub fn concurrency(&self) -> usize {
        self.conc.load(Ordering::SeqCst).max(1)
    }

    pub fn running(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    /// 拿一个下载名额：名额满了就等着。用轮询而不是信号量，
    /// 因为上限随时可能被调小 —— 信号量发出去的许可收不回来。
    async fn acquire_slot(&self) -> usize {
        loop {
            let cur = self.running.load(Ordering::SeqCst);
            let cap = self.concurrency();
            if cur < cap {
                if self
                    .running
                    .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    return cur + 1;
                }
                continue;
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
    }

    fn release_slot(&self) {
        let _ = self
            .running
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            });
    }

    pub fn list(&self) -> Vec<Job> {
        let mut v: Vec<Job> = self
            .inner
            .lock()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default();
        // 活跃在前，其次按创建时间倒序
        v.sort_by(|a, b| {
            let ra = if a.status.is_active() { 0 } else { 1 };
            let rb = if b.status.is_active() { 0 } else { 1 };
            ra.cmp(&rb).then(b.created_at.cmp(&a.created_at))
        });
        v
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.inner.lock().ok()?.get(id).cloned()
    }

    fn snapshot(&self, id: &str) -> Option<Job> {
        self.get(id)
    }

    fn broadcast(&self, id: &str) {
        if let Some(job) = self.snapshot(id) {
            let _ = self.tx.send(job);
        }
    }

    fn update<F: FnOnce(&mut Job)>(&self, id: &str, f: F) {
        if let Ok(mut m) = self.inner.lock() {
            if let Some(job) = m.get_mut(id) {
                f(job);
            }
        }
        self.broadcast(id);
    }

    pub fn logs(&self, id: &str) -> Vec<String> {
        self.logs
            .lock()
            .ok()
            .and_then(|m| m.get(id).cloned())
            .unwrap_or_default()
    }

    fn push_log(&self, id: &str, line: String) {
        if let Ok(mut m) = self.logs.lock() {
            let e = m.entry(id.to_string()).or_default();
            e.push(line);
            if e.len() > 300 {
                e.remove(0);
            }
        }
    }

    pub fn cancel(&self, id: &str) -> bool {
        let flag = self.cancel.lock().ok().and_then(|m| m.get(id).cloned());
        match flag {
            Some(f) => {
                f.store(true, Ordering::SeqCst);
                // 还没启动的排队任务直接落定为已取消
                let mut was_queued = false;
                if let Ok(mut m) = self.inner.lock() {
                    if let Some(job) = m.get_mut(id) {
                        if job.status == Status::Queued {
                            job.status = Status::Cancelled;
                            job.ended_at = Some(now_ms());
                            was_queued = true;
                        }
                    }
                }
                if was_queued {
                    self.broadcast(id);
                }
                true
            }
            None => false,
        }
    }

    /// 按已有任务的参数重开一个任务（失败 / 取消之后重试）。
    /// 直接复用原任务的 url / 模式 / 画质 / 格式 ID / 播放列表开关，
    /// 用户不用回去重新解析一遍 —— 网络抖一下就得重填 URL 太难受了。
    pub fn respawn(self: &Arc<Self>, id: &str) -> Result<String, String> {
        let old = self.get(id).ok_or_else(|| format!("任务不存在: {id}"))?;
        if old.status.is_active() {
            return Err("任务还在进行中，不需要重试".into());
        }
        self.spawn(NewJob {
            url: old.url,
            mode: old.mode,
            quality: old.quality,
            format_id: old.format_id,
            playlist: old.playlist,
            direct: old.direct,
            direct_title: old.direct_title,
            settings: crate::settings::load(),
        })
    }

    /// 建任务并立刻返回 id；真正的下载在后台跑
    pub fn spawn(self: &Arc<Self>, req: NewJob) -> Result<String, String> {
        let out_dir = crate::settings::ensure_output_dir(&req.settings)?;
        let n = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("job-{}-{}", now_ms(), n);

        let job = Job {
            id: id.clone(),
            url: req.url.clone(),
            title: String::new(),
            mode: req.mode.clone(),
            quality: req.quality.clone(),
            format_id: req.format_id.clone(),
            playlist: req.playlist,
            direct: req.direct.clone(),
            direct_title: req.direct_title.clone(),
            status: Status::Queued,
            percent: 0.0,
            downloaded_bytes: 0,
            total_bytes: None,
            speed: None,
            eta: None,
            output: None,
            file_name: None,
            error: None,
            playlist_index: None,
            playlist_count: None,
            warnings: Vec::new(),
            created_at: now_ms(),
            ended_at: None,
        };
        if let Ok(mut m) = self.inner.lock() {
            m.insert(id.clone(), job.clone());
        }
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut m) = self.cancel.lock() {
            m.insert(id.clone(), flag.clone());
        }
        self.broadcast(&id);

        let opts = DownloadOpts {
            url: req.url,
            mode: req.mode,
            quality: req.quality,
            format_id: req.format_id,
            playlist: req.playlist,
            output_dir: out_dir,
            cookies_from_browser: req.settings.cookies_from_browser.clone(),
            proxy: req.settings.proxy.clone(),
            impersonate: req.settings.impersonate.clone(),
            direct: req.direct,
            direct_title: req.direct_title,
        };

        let store = self.clone();
        let jid = id.clone();
        // 发布用：配置快照 + 本次任务产出的所有成品路径。
        // 为什么要在 Event::File 里收集而不是用 download_blocking 的返回值：
        // 播放列表只返回最后一项，靠返回值会漏掉前面的成品。
        let publish_settings = req.settings.clone();
        let publish_files: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let publish_files_cb = publish_files.clone();
        tokio::spawn(async move {
            store.acquire_slot().await;
            let _guard = SlotGuard(store.clone());
            if flag.load(Ordering::SeqCst) {
                store.update(&jid, |j| {
                    j.status = Status::Cancelled;
                    j.ended_at = Some(now_ms());
                });
                return;
            }
            store.update(&jid, |j| j.status = Status::Running);

            let s2 = store.clone();
            let j2 = jid.clone();
            let result = tokio::task::spawn_blocking(move || {
                ytdlp::download_blocking(&opts, flag.clone(), |ev| match ev {
                    Event::Title {
                        index,
                        count,
                        title,
                    } => s2.update(&j2, |j| {
                        // 播放列表：标题始终跟着「当前这一项」走，让人看到正在下哪个
                        j.title = title;
                        if index.is_some() {
                            j.playlist_index = index;
                        }
                        if count.is_some() {
                            j.playlist_count = count;
                        }
                    }),
                    Event::Prog {
                        downloaded,
                        total,
                        speed,
                        eta,
                        index,
                    } => s2.update(&j2, |j| {
                        if index.is_some() {
                            j.playlist_index = index;
                        }
                        j.downloaded_bytes = downloaded;
                        j.total_bytes = total;
                        j.speed = speed;
                        j.eta = eta;
                        let item = match total {
                            Some(t) if t > 0 => (downloaded as f64 / t as f64).clamp(0.0, 1.0),
                            _ => 0.0,
                        };
                        // 播放列表按「已完成项 + 当前项进度」折算整体百分比，
                        // 否则每换一项进度条就从 100% 掉回 0%，看不出整体推进。
                        // 再取 max：yt-dlp 在换项瞬间会吐出 total=NA 的残留行（download 还是上一项的字节数），
                        // 不夹住的话整体进度会突然往回掉。
                        let is_playlist =
                            matches!((index, j.playlist_count), (Some(_), Some(n)) if n > 0);
                        let cand = match (index, j.playlist_count) {
                            (Some(i), Some(n)) if n > 0 => {
                                let done = (i.saturating_sub(1)) as f64;
                                (((done + item) / n as f64) * 100.0 * 10.0).round() / 10.0
                            }
                            _ => match total {
                                Some(t) if t > 0 => (item * 100.0 * 10.0).round() / 10.0,
                                _ => j.percent,
                            },
                        };
                        j.percent = if is_playlist {
                            j.percent.max(cand)
                        } else {
                            cand
                        };
                    }),
                    Event::File(p) => {
                        let pb = PathBuf::from(&p);
                        let parent = pb
                            .parent()
                            .map(|d| d.display().to_string())
                            .unwrap_or_default();
                        let name = pb
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        s2.update(&j2, |j| {
                            j.output = Some(parent);
                            j.file_name = Some(name);
                        });
                        if let Ok(mut v) = publish_files_cb.lock() {
                            if !v.iter().any(|x| x == &p) {
                                v.push(p);
                            }
                        }
                    }
                    Event::Warn(w) => {
                        s2.push_log(&j2, format!("WARN: {w}"));
                        s2.update(&j2, |j| {
                            if !j.warnings.iter().any(|x| x == &w) {
                                j.warnings.push(w);
                                // 上千项的播放列表别把内存撑爆
                                if j.warnings.len() > 50 {
                                    j.warnings.remove(0);
                                }
                            }
                        });
                    }
                    Event::Log(l) => s2.push_log(&j2, l),
                })
            })
            .await;

            match result {
                Ok(Ok(file)) => {
                    let pb = PathBuf::from(&file);
                    let parent = pb
                        .parent()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    let name = pb
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    store.update(&jid, |j| {
                        j.status = Status::Done;
                        j.percent = 100.0;
                        j.speed = None;
                        j.eta = None;
                        // 完成时以成品文件的真实大小为准：音频抽取会把 mp4 转成 mp3，
                        // 用下载阶段的 total_bytes 会显示成「0 B / 2.7 MB」这种自相矛盾的数字
                        let mut size = std::fs::metadata(&file).map(|m| m.len() as i64).ok();
                        if size.unwrap_or(0) <= 0 {
                            size = j.total_bytes;
                        }
                        if let Some(n) = size {
                            j.downloaded_bytes = n;
                            j.total_bytes = Some(n);
                        }
                        if !parent.is_empty() {
                            j.output = Some(parent);
                        }
                        if !name.is_empty() {
                            j.file_name = Some(name);
                        }
                        j.ended_at = Some(now_ms());
                    });
                    store.push_log(&jid, "任务完成".into());
                    spawn_publish(&store, &jid, &file, &publish_settings, &publish_files);
                }
                Ok(Err(e)) => {
                    let cancelled = e.contains("已取消") || e.contains("cancel");
                    store.update(&jid, |j| {
                        j.status = if cancelled {
                            Status::Cancelled
                        } else {
                            Status::Failed
                        };
                        j.error = if cancelled { None } else { Some(e.clone()) };
                        j.speed = None;
                        j.eta = None;
                        j.ended_at = Some(now_ms());
                    });
                    store.push_log(
                        &jid,
                        if cancelled {
                            "任务已取消".to_string()
                        } else {
                            format!("任务失败：{e}")
                        },
                    );
                }
                Err(join_err) => {
                    store.update(&jid, |j| {
                        j.status = Status::Failed;
                        j.error = Some(format!("下载线程崩溃: {join_err}"));
                        j.ended_at = Some(now_ms());
                    });
                    store.push_log(&jid, format!("下载线程崩溃：{join_err}"));
                }
            }
            if let Ok(mut m) = store.cancel.lock() {
                m.remove(&jid);
            }
            store.prune();
        });

        Ok(id)
    }

    /// 清理已结束任务的记录（保留最近 200 条历史）
    pub fn prune(&self) {
        let mut kill: Vec<String> = Vec::new();
        if let Ok(m) = self.inner.lock() {
            let mut done: Vec<&Job> = m.values().filter(|j| !j.status.is_active()).collect();
            done.sort_by_key(|a| std::cmp::Reverse(a.created_at));
            for j in done.into_iter().skip(200) {
                kill.push(j.id.clone());
            }
        }
        if kill.is_empty() {
            return;
        }
        if let Ok(mut m) = self.inner.lock() {
            for id in &kill {
                m.remove(id);
            }
        }
        if let Ok(mut m) = self.logs.lock() {
            for id in &kill {
                m.remove(id);
            }
        }
    }
}
