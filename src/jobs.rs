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

use crate::ytdlp::{self, DownloadOpts, Event};

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
    sem: Arc<tokio::sync::Semaphore>,
    /// 当前并发上限（并发只增不减；调小需重启生效）
    conc: Arc<std::sync::atomic::AtomicUsize>,
    seq: Arc<AtomicU64>,
    /// 日志尾部（每个任务最近若干行），用于失败排查
    logs: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

pub struct NewJob {
    pub url: String,
    pub mode: String,
    pub quality: String,
    pub format_id: Option<String>,
    pub playlist: bool,
    pub settings: crate::settings::Settings,
}

impl JobStore {
    pub fn new(concurrency: usize) -> Self {
        let (tx, _rx) = broadcast::channel(2048);
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            cancel: Arc::new(Mutex::new(HashMap::new())),
            tx,
            sem: Arc::new(tokio::sync::Semaphore::new(concurrency.max(1))),
            conc: Arc::new(std::sync::atomic::AtomicUsize::new(concurrency.max(1))),
            seq: Arc::new(AtomicU64::new(0)),
            logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Job> {
        self.tx.subscribe()
    }

    /// 调整并发上限：调大立即生效（补发 permits），调小等重启再生效
    pub fn set_concurrency(&self, n: usize) {
        let want = n.max(1);
        let old = self.conc.swap(want, Ordering::SeqCst);
        if want > old {
            self.sem.add_permits(want - old);
        }
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
            status: Status::Queued,
            percent: 0.0,
            downloaded_bytes: 0,
            total_bytes: None,
            speed: None,
            eta: None,
            output: None,
            file_name: None,
            error: None,
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
        };

        let store = self.clone();
        let jid = id.clone();
        tokio::spawn(async move {
            let permit = store.sem.clone().acquire_owned().await;
            let _permit = match permit {
                Ok(p) => p,
                Err(_) => return,
            };
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
                    Event::Title(t) => s2.update(&j2, |j| {
                        if j.title.is_empty() {
                            j.title = t;
                        }
                    }),
                    Event::Prog {
                        downloaded,
                        total,
                        speed,
                        eta,
                    } => s2.update(&j2, |j| {
                        j.downloaded_bytes = downloaded;
                        j.total_bytes = total;
                        j.speed = speed;
                        j.eta = eta;
                        j.percent = match total {
                            Some(t) if t > 0 => {
                                ((downloaded as f64 / t as f64) * 100.0 * 10.0).round() / 10.0
                            }
                            _ => j.percent,
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
                }
                Err(join_err) => {
                    store.update(&jid, |j| {
                        j.status = Status::Failed;
                        j.error = Some(format!("下载线程崩溃: {join_err}"));
                        j.ended_at = Some(now_ms());
                    });
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
            done.sort_by(|a, b| b.created_at.cmp(&a.created_at));
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
