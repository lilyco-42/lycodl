//! HTTP 控制台：给 Web UI 用的 REST + SSE 接口。
//!
//! 与 CLI / TUI / MCP 并列的第四个前端 —— 只是这里的前端是浏览器。
//! 后端只暴露三样东西：yt-dlp 内核、任务表、内嵌的前端产物。

use std::collections::HashMap;
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::jobs::{Job, JobStore, NewJob};
use crate::settings::{self, Settings};
use crate::ytdlp;

// ── 错误形态 ───────────────────────────────────────────────────

/// 统一错误体：{"error": "..."} —— 前端 readErrorPayload 只认这个字段。
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }
    fn not_found(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::NOT_FOUND, message: msg.into() }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: msg.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

// ── 共享状态 ───────────────────────────────────────────────────

/// 上游最新内核版本（后台定时探测，前端不必每次都等 GitHub 5~10 秒）
#[derive(Clone, Default)]
pub struct KernelWatch {
    inner: Arc<RwLock<Option<Latest>>>,
}

#[derive(Clone)]
pub struct Latest {
    pub version: String,
    /// 毫秒时间戳
    pub checked_at: u64,
}

impl KernelWatch {
    pub fn get(&self) -> Option<Latest> {
        self.inner.read().ok().and_then(|g| g.clone())
    }

    pub fn set(&self, version: String) {
        let at = now_ms();
        if let Ok(mut w) = self.inner.write() {
            *w = Some(Latest { version, checked_at: at });
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<JobStore>,
    /// 配置的内存副本（改配置立刻对新任务生效）
    pub settings: Arc<RwLock<Settings>>,
    /// 上游内核版本缓存（热更新红利的「红利」一半靠它：不用用户去点才告诉你有没有新版）
    pub kernel: KernelWatch,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        let store = JobStore::new(settings.concurrency.max(1));
        Self {
            store: Arc::new(store),
            settings: Arc::new(RwLock::new(settings)),
            kernel: KernelWatch::default(),
        }
    }

    fn snapshot(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }
}

// ── 路由 ───────────────────────────────────────────────────────

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/ytdlp", get(ytdlp_info))
        .route("/api/ytdlp/update", post(ytdlp_update))
        .route("/api/probe", post(probe))
        .route("/api/download", post(download))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}/events", get(job_events))
        .route("/api/jobs/{id}/cancel", post(cancel_job))
        .route("/api/jobs/{id}/logs", get(job_logs))
        .route("/api/files", get(list_files))
        .route("/api/files/{name}", get(serve_file))
        .route("/api/commands", get(list_commands))
        .with_state(state)
        .fallback(static_file)
}

// ── 元信息 ─────────────────────────────────────────────────────

async fn health() -> Json<Value> {
    let kernel = ytdlp::locate().ok();
    Json(json!({
        "name": "lycodl",
        "version": env!("CARGO_PKG_VERSION"),
        "ytdlpVersion": kernel.as_ref().map(|k| k.version.clone()),
        "ytdlpPath": kernel.as_ref().map(|k| k.path.display().to_string()),
        "webMode": if crate::assets::embedded() { "embedded" } else { "disk" },
    }))
}

async fn get_settings(State(st): State<AppState>) -> Json<Settings> {
    Json(st.snapshot())
}

/// 前端发的是 Partial patch：逐字段覆盖现有配置
async fn put_settings(
    State(st): State<AppState>,
    Json(patch): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let mut next = st.snapshot();
    if let Some(v) = patch.get("outputDir").and_then(|v| v.as_str()) {
        let t = v.trim();
        if !t.is_empty() {
            next.output_dir = t.to_string();
        }
    }
    if let Some(v) = patch.get("cookiesFromBrowser") {
        next.cookies_from_browser = match v.as_str() {
            Some(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        };
    }
    if let Some(v) = patch.get("proxy") {
        next.proxy = match v.as_str() {
            Some(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        };
    }
    if let Some(v) = patch.get("concurrency").and_then(|v| v.as_u64()) {
        next.concurrency = (v as usize).max(1);
    }

    settings::save(&next).map_err(ApiError::internal)?;
    settings::ensure_output_dir(&next).map_err(ApiError::bad)?;
    if let Ok(mut w) = st.settings.write() {
        *w = next.clone();
    }
    st.store.set_concurrency(next.concurrency);
    Ok(Json(json!({ "ok": true, "settings": next })))
}

async fn ytdlp_info(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let check = q.get("check").map(|v| v == "1" || v == "true").unwrap_or(false);
    match ytdlp::locate() {
        Ok(k) => {
            let mut latest: Option<String> = None;
            let mut error: Option<String> = None;
            if check {
                // 用户主动点「检查更新」：当场问一次 GitHub，并回填缓存
                match ytdlp::latest_version().await {
                    Ok(v) => {
                        st.kernel.set(v.clone());
                        latest = Some(v);
                    }
                    Err(e) => error = Some(e),
                }
            } else {
                // 平时直接给后台探到的结果，页面一打开就知道有没有新版
                latest = st.kernel.get().map(|l| l.version);
            }
            let latest_checked_at = st.kernel.get().map(|l| l.checked_at);
            let update_available = match &latest {
                Some(l) => l != &k.version,
                None => false,
            };
            Json(json!({
                "path": k.path.display().to_string(),
                "version": k.version,
                "source": k.source,
                "latest": latest,
                "latestCheckedAt": latest_checked_at,
                "updateAvailable": update_available,
                "error": error,
            }))
        }
        Err(e) => Json(json!({
            "path": "",
            "version": "",
            "source": "system",
            "latest": null,
            "updateAvailable": false,
            "error": e,
        })),
    }
}

async fn ytdlp_update() -> Result<Json<Value>, ApiError> {
    match ytdlp::update().await {
        Ok((from, to, path)) => Ok(Json(json!({
            "ok": true,
            "from": from,
            "alreadyLatest": from == to,
            "to": to,
            "path": path.display().to_string(),
        }))),
        Err(e) => Err(ApiError::internal(e)),
    }
}

// ── 解析 ───────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ProbeReq {
    url: String,
}

async fn probe(State(st): State<AppState>, Json(req): Json<ProbeReq>) -> Result<Json<Value>, ApiError> {
    let url = req.url.trim().to_string();
    if url.is_empty() {
        return Err(ApiError::bad("链接不能为空"));
    }
    let s = st.snapshot();
    let info = tokio::task::spawn_blocking(move || {
        ytdlp::probe(&url, s.cookies_from_browser.as_deref(), s.proxy.as_deref())
    })
    .await
    .map_err(|e| ApiError::internal(format!("解析线程崩溃: {e}")))?
    .map_err(ApiError::bad)?;
    Ok(Json(json!({ "ok": true, "info": info })))
}

// ── 下载 ───────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadReq {
    url: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_quality")]
    quality: String,
    #[serde(default)]
    format_id: Option<String>,
    #[serde(default)]
    playlist: bool,
}

fn default_mode() -> String {
    "video".into()
}

fn default_quality() -> String {
    "best".into()
}

async fn download(State(st): State<AppState>, Json(req): Json<DownloadReq>) -> Result<Json<Value>, ApiError> {
    let url = req.url.trim().to_string();
    if url.is_empty() {
        return Err(ApiError::bad("链接不能为空"));
    }
    if !matches!(req.mode.as_str(), "video" | "audio" | "custom") {
        return Err(ApiError::bad(format!("未知下载模式: {}", req.mode)));
    }
    if req.mode == "custom" && req.format_id.as_deref().unwrap_or("").trim().is_empty() {
        return Err(ApiError::bad("自定义模式必须指定 formatId"));
    }
    let job_id = st
        .store
        .spawn(NewJob {
            url,
            mode: req.mode,
            quality: req.quality,
            format_id: req.format_id,
            playlist: req.playlist,
            settings: st.snapshot(),
        })
        .map_err(ApiError::bad)?;
    Ok(Json(json!({ "ok": true, "jobId": job_id })))
}

async fn list_jobs(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "jobs": st.store.list() }))
}

async fn cancel_job(State(st): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    if st.store.cancel(&id) {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(ApiError::not_found(format!("任务不存在: {id}")))
    }
}

/// 任务的 yt-dlp 原始日志（排障用）
async fn job_logs(State(st): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "id": id, "logs": st.store.logs(&id) }))
}

/// SSE：每任务一条流，事件体就是裸 Job JSON（前端 onmessage 直接 JSON.parse）
async fn job_events(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<SseEvent, std::convert::Infallible>>>, ApiError> {
    let first = st
        .store
        .get(&id)
        .ok_or_else(|| ApiError::not_found(format!("任务不存在: {id}")))?;
    let mut rx = st.store.subscribe();
    let stream = async_stream::stream! {
        let finished = !first.status.is_active();
        yield Ok(sse_data(&first));
        if finished {
            return;
        }
        loop {
            match rx.recv().await {
                Ok(job) => {
                    if job.id != id {
                        continue;
                    }
                    let terminal = !job.status.is_active();
                    yield Ok(sse_data(&job));
                    if terminal {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn sse_data(job: &Job) -> SseEvent {
    SseEvent::default().data(serde_json::to_string(job).unwrap_or_else(|_| "{}".into()))
}

// ── 文件 ───────────────────────────────────────────────────────

async fn list_files(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let s = st.snapshot();
    let dir = settings::ensure_output_dir(&s).map_err(ApiError::bad)?;
    let rd = std::fs::read_dir(&dir)
        .map_err(|e| ApiError::bad(format!("读取 {} 失败: {e}", dir.display())))?;
    let mut items: Vec<Value> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let name = match path.file_name().map(|n| n.to_string_lossy().to_string()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        // yt-dlp 的中间文件（.part / .ytdl / .temp）不当作成品展示
        if [".part", ".ytdl", ".temp"].iter().any(|s| name.ends_with(s)) {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        items.push(json!({
            "name": name,
            "size": meta.len(),
            "modified": modified,
            "url": format!("/api/files/{}", urlencode(&name)),
        }));
    }
    items.sort_by(|a, b| {
        let am = a.get("modified").and_then(|v| v.as_u64()).unwrap_or(0);
        let bm = b.get("modified").and_then(|v| v.as_u64()).unwrap_or(0);
        bm.cmp(&am)
    });
    Ok(Json(json!({ "files": items })))
}

async fn serve_file(State(st): State<AppState>, Path(name): Path<String>) -> Result<Response, ApiError> {
    if name.contains('/') || name.contains("..") || name.trim().is_empty() {
        return Err(ApiError::bad("文件名非法"));
    }
    let s = st.snapshot();
    let mut path = PathBuf::from(s.output_dir.trim());
    path.push(&name);
    if !path.is_file() {
        return Err(ApiError::not_found(format!("文件不存在: {name}")));
    }
    let bytes = std::fs::read(&path).map_err(|e| ApiError::internal(format!("读取文件失败: {e}")))?;
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let disp = if name.chars().all(|c| c.is_ascii_graphic() || c == ' ') && !name.contains('"') {
        format!("attachment; filename=\"{name}\"")
    } else {
        // 非 ASCII 文件名走 RFC 5987，保留原始名字（浏览器会优先用它）
        format!(
            "attachment; filename=\"{}\"; filename*=UTF-8''{}",
            ascii_filename(&name),
            urlencode(&name)
        )
    };
    let mut resp = ([(header::CONTENT_TYPE, mime.as_ref().to_string())], bytes).into_response();
    if let Ok(v) = HeaderValue::from_str(&disp) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

fn ascii_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ── 命令清单（给「命令 / AI」页展示能力面）──────────────────────

async fn list_commands() -> Json<Value> {
    let reg = crate::commands::registry();
    let mut list: Vec<Value> = Vec::new();
    for cmd in reg.visible() {
        let args: Vec<Value> = cmd
            .schema
            .args
            .iter()
            .map(|a| {
                json!({
                    "name": a.name,
                    "about": a.about,
                    "required": a.required,
                    "kind": serde_json::to_value(&a.kind).unwrap_or_else(|_| json!({ "type": "text" })),
                })
            })
            .collect();
        list.push(json!({
            "name": cmd.name,
            "about": cmd.schema.about,
            "safety": safety_name(cmd.schema.safety),
            "args": args,
        }));
    }
    Json(json!({ "commands": list }))
}

/// SafetyTier 的 serde 名是 snake_case，前端契约要 PascalCase —— 手工映射
fn safety_name(t: lilyco::prelude::SafetyTier) -> &'static str {
    match t {
        lilyco::prelude::SafetyTier::ReadOnly => "ReadOnly",
        lilyco::prelude::SafetyTier::Confirm => "Confirm",
        lilyco::prelude::SafetyTier::Token => "Token",
        lilyco::prelude::SafetyTier::NeverAuto => "NeverAuto",
    }
}

// ── 静态资源（前端产物已编进二进制）────────────────────────────

async fn static_file(uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "未知接口" }))).into_response();
    }
    match crate::assets::get(path) {
        Some(f) => {
            // 无扩展名的路径（根路径 / 客户端路由）一律当 HTML，避免浏览器当附件下载
            let mime = if StdPath::new(path).extension().is_none() {
                mime_guess::mime::TEXT_HTML_UTF_8
            } else {
                mime_guess::from_path(path).first_or_octet_stream()
            };
            let cache: &'static str = if path.starts_with("/assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            let mut resp = (StatusCode::OK, f.contents().to_vec()).into_response();
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime.as_ref())
                    .unwrap_or(HeaderValue::from_static("application/octet-stream")),
            );
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
            resp
        }
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "页面不存在" }))).into_response(),
    }
}

// ── 启动 ───────────────────────────────────────────────────────

fn state_file() -> std::path::PathBuf {
    crate::paths::data_dir().join("web.port")
}

/// 读上次用过的端口（可能已失效，只是个提示）
fn remembered_port() -> Option<u16> {
    std::fs::read_to_string(state_file())
        .ok()
        .and_then(|t| t.trim().parse::<u16>().ok())
}

fn remember_port(port: u16) {
    if let Some(dir) = state_file().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(state_file(), port.to_string());
}

/// 那个端口上是不是已经有一个活着的 lycodl？（认 /api/health 里的名字，别认错别的程序）
async fn alive_at(port: u16) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(900))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let url = format!("http://127.0.0.1:{port}/api/health");
    match client.get(&url).send().await {
        Ok(r) => match r.json::<serde_json::Value>().await {
            Ok(v) => v.get("name").and_then(|n| n.as_str()) == Some("lycodl"),
            Err(_) => false,
        },
        Err(_) => false,
    }
}

fn port() -> u16 {
    for key in ["LYCODL_PORT", "LILYCO_PORT"] {
        if let Ok(v) = std::env::var(key) {
            if let Ok(p) = v.parse::<u16>() {
                return p;
            }
        }
    }
    8765
}

/// 起 Web 控制台（阻塞到进程退出）
pub fn serve() {
    let s = settings::load();
    let port = port();
    let state = AppState::new(s);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async move {
        // 已经开着一个就把浏览器指过去，别在同一台机器上叠出第二个服务
        if let Some(p) = remembered_port() {
            if alive_at(p).await {
                let url = format!("http://127.0.0.1:{p}");
                println!("lycodl 已在运行: {url}");
                maybe_open_browser(&url);
                return;
            }
        }
        // 后台探上游版本：启动时一次，之后每 6 小时一次；失败了下次再试，绝不打扰用户
        {
            let watch = state.kernel.clone();
            tokio::spawn(async move {
                loop {
                    match ytdlp::latest_version().await {
                        Ok(v) => watch.set(v),
                        Err(e) => eprintln!("lycodl: 检查内核更新失败（稍后重试）: {e}"),
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
                }
            });
        }
        let app = router(state);
        // 端口可能被别的程序占着，逐个往后找，别让用户对着报错干瞪眼
        let mut listener = None;
        let mut chosen = port;
        let mut last_err = String::new();
        for candidate in port..port.saturating_add(20) {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], candidate));
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => {
                    listener = Some(l);
                    chosen = candidate;
                    break;
                }
                Err(e) => last_err = format!("{e}"),
            }
        }
        let Some(listener) = listener else {
            eprintln!("lycodl: 绑定 127.0.0.1:{port}..{} 全部失败: {last_err}", port.saturating_add(20));
            std::process::exit(1);
        };
        let port = chosen;
        let url = format!("http://127.0.0.1:{port}");
        remember_port(port);
        println!("lycodl Web 控制台: {url}");
        if crate::assets::embedded() {
            println!("前端已编入二进制（webMode=embedded）");
        } else {
            println!("警告：web/dist 为空，页面不可用（请先在 web/ 里 pnpm build）");
        }
        maybe_open_browser(&url);
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("lycodl: HTTP 服务异常退出: {e}");
        }
    });
}

/// 有图形会话时顺手开浏览器（LYCODL_NO_OPEN=1 可关闭）
fn maybe_open_browser(url: &str) {
    if std::env::var("LYCODL_NO_OPEN").map(|v| v == "1").unwrap_or(false) {
        return;
    }
    let has_display =
        std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok();
    if !has_display {
        return;
    }
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// 给测试用：只拿 Router 不自起监听
#[allow(dead_code)]
pub fn app_for_test(dir: &StdPath) -> Router {
    let mut s = Settings::default();
    s.output_dir = dir.display().to_string();
    router(AppState::new(s))
}
