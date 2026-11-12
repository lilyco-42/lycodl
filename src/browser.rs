//! 浏览器桥：用本机 Chromium 真正打开页面，把「页面能看、yt-dlp 拿不到」的东西带出来
//!
//! 为什么需要它：yt-dlp 的 extractor 是逆向出来的，站点一改版就断（抖音/小红书/reddit 这些
//! 就是活例子）。而浏览器永远是对的 —— 它加载的页面里就有播放器真实在用的地址。
//!
//! 这里做两件事：
//! 1. 拿 Cookie：把浏览器挣到的 cookie 导出成 Netscape 文件喂给 yt-dlp（微博靠这个解锁）；
//! 2. 嗅媒体：记录页面实际请求的媒体地址，extractor 全挂时用它直接下（通用兜底）。
//!
//! 全部走 CDP（DevTools 协议），不依赖 puppeteer / playwright 之类 Node 生态。

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 浏览器挣到的战利品
#[derive(Debug, Clone, Default)]
pub struct BridgeOutcome {
    /// 导出的 Netscape cookie 文件（页面一个 cookie 都没种下时为 None）
    pub cookies_file: Option<PathBuf>,
    pub user_agent: String,
    pub title: String,
    /// 反爬挑战（Cloudflare 之类）是否已放行
    pub challenge_passed: bool,
    pub media: Vec<MediaHit>,
}

#[derive(Debug, Clone)]
pub struct MediaHit {
    pub url: String,
    pub mime: String,
    pub size: u64,
}

/// 找一个能用的 Chromium 系浏览器
pub fn chromium_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LYCODL_BROWSER") {
        let p = p.trim();
        if !p.is_empty() && Path::new(p).is_file() {
            return Some(PathBuf::from(p));
        }
    }
    for name in ["chromium", "chromium-browser", "google-chrome-stable", "google-chrome", "microsoft-edge-stable", "brave-browser"] {
        if let Some(p) = which(name) {
            return Some(p);
        }
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// 打开一个页面，等它稳定下来，把 cookie 和媒体地址一起带回来
pub fn bridge(url: &str, wait_ms: u64) -> Result<BridgeOutcome, String> {
    let bin = chromium_path().ok_or_else(|| {
        "没找到 Chromium 系浏览器：装一个 chromium，或用 LYCODL_BROWSER 指定可执行文件".to_string()
    })?;
    let prof = std::env::temp_dir().join(format!("lycodl-bridge-{}", now_ms()));
    std::fs::create_dir_all(&prof).map_err(|e| format!("创建浏览器配置目录失败: {e}"))?;

    let mut cmd = Command::new(&bin);
    cmd.arg("--headless=new")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", prof.display()))
        .arg("--no-sandbox")
        .arg("--disable-gpu")
        .arg("--mute-audio")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--autoplay-policy=no-user-gesture-required")
        .arg("--window-size=1280,900")
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("启动浏览器失败: {e}"))?;

    let result = run_bridge(&prof, url, wait_ms);

    // 整组杀掉：chromium 会 fork 出 zygote/renderer，只杀父进程会留下孤儿
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&prof);
    result
}

fn run_bridge(prof: &Path, url: &str, wait_ms: u64) -> Result<BridgeOutcome, String> {
    let port = wait_devtools_port(prof, Duration::from_secs(15))?;
    let targets = http_get(&format!("http://127.0.0.1:{port}/json/list"))?;
    let targets: Value = serde_json::from_str(&targets).map_err(|e| format!("解析调试目标失败: {e}"))?;
    let ws_url = targets
        .as_array()
        .and_then(|a| a.iter().find(|t| t.get("type").and_then(|v| v.as_str()) == Some("page")))
        .and_then(|t| t.get("webSocketDebuggerUrl").and_then(|v| v.as_str()))
        .ok_or_else(|| "浏览器没有可用的页面目标".to_string())?
        .to_string();

    let (mut ws, _) = tungstenite::connect(&ws_url).map_err(|e| format!("连接浏览器调试端口失败: {e}"))?;
    let mut rpc = Rpc::new();
    rpc.call(&mut ws, "Network.enable", json!({}))?;
    rpc.call(&mut ws, "Page.enable", json!({}))?;
    rpc.call(&mut ws, "Runtime.enable", json!({}))?;
    let ua = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36";
    rpc.call(&mut ws, "Network.setUserAgentOverride", json!({ "userAgent": ua }))?;
    rpc.call(&mut ws, "Page.navigate", json!({ "url": url }))?;

    let deadline = Instant::now() + Duration::from_millis(wait_ms.max(3000));
    let mut media: Vec<MediaHit> = Vec::new();
    let mut title = String::new();
    let mut challenge_passed = false;
    let mut videos = 0i64;
    let mut src = String::new();

    while Instant::now() < deadline {
        rpc.drain(&mut ws, &mut media);
        std::thread::sleep(Duration::from_millis(700));
        let probe = "JSON.stringify({t:document.title,n:document.body?document.body.innerText.length:0,\
                     vv:document.querySelectorAll('video').length,\
                     v:(document.querySelector('video')||{}).currentSrc||''})";
        let probe = probe.replace("\\\n", "");
        if let Ok(v) = rpc.eval(&mut ws, &probe) {
            title = v.get("t").and_then(|x| x.as_str()).unwrap_or("").to_string();
            videos = v.get("vv").and_then(|x| x.as_i64()).unwrap_or(0);
            src = v.get("v").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let blocked = is_challenge_title(&title);
            if !blocked {
                challenge_passed = true;
                if !src.is_empty() || !media.is_empty() {
                    break;
                }
            }
        }
    }
    // 让播放器真正开始拉流（很多站点是点播放才请求媒体）
    if videos > 0 {
        let _ = rpc.eval(
            &mut ws,
            "(()=>{const vs=[...document.querySelectorAll('video')];vs.forEach(v=>{try{v.muted=true;v.play()}catch(e){}});return vs.length})()",
        );
        let extra = Instant::now() + Duration::from_millis(3500);
        while Instant::now() < extra {
            rpc.drain(&mut ws, &mut media);
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    if let Ok(v) = rpc.eval(&mut ws, "JSON.stringify({t:document.title,v:(document.querySelector('video')||{}).currentSrc||''})") {
        if let Some(t) = v.get("t").and_then(|x| x.as_str()) {
            if !t.is_empty() {
                title = t.to_string();
            }
        }
        if let Some(s) = v.get("v").and_then(|x| x.as_str()) {
            if !s.is_empty() {
                src = s.to_string();
            }
        }
    }

    let mut media = media;
    if !src.is_empty() && !media.iter().any(|m| m.url == src) {
        media.insert(
            0,
            MediaHit {
                url: src,
                mime: "video/mp4".into(),
                size: 0,
            },
        );
    }
    let cookies = rpc.cookies(&mut ws).unwrap_or_default();
    let _ = ws.close(None);

    let cookies_file = write_netscape(&cookies);
    Ok(BridgeOutcome {
        cookies_file,
        user_agent: ua.to_string(),
        title,
        challenge_passed,
        media,
    })
}

fn is_challenge_title(t: &str) -> bool {
    let low = t.to_lowercase();
    [
        "请稍候",
        "just a moment",
        "checking your browser",
        "attention required",
        "ddos-guard",
        "verifying you are human",
        "请稍等",
    ]
    .iter()
    .any(|k| low.contains(k))
}

fn wait_devtools_port(prof: &Path, timeout: Duration) -> Result<u16, String> {
    let f = prof.join("DevToolsActivePort");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(t) = std::fs::read_to_string(&f) {
            if let Some(line) = t.lines().next() {
                if let Ok(p) = line.trim().parse::<u16>() {
                    if p > 0 {
                        return Ok(p);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("浏览器调试端口没起来（可能被系统策略禁用）".into())
}

fn write_netscape(cookies: &[(String, String, String, String, bool, f64)]) -> Option<PathBuf> {
    if cookies.is_empty() {
        return None;
    }
    let mut body = String::from("# Netscape HTTP Cookie File\n");
    for (domain, path, name, value, secure, expires) in cookies {
        let inc = if domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let exp = if *expires > 0.0 { *expires as i64 } else { 0 };
        body.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            domain,
            inc,
            if path.is_empty() { "/" } else { path },
            if *secure { "TRUE" } else { "FALSE" },
            exp,
            name,
            value
        ));
    }
    let p = std::env::temp_dir().join(format!("lycodl-cookies-{}.txt", now_ms()));
    std::fs::write(&p, body).ok()?;
    Some(p)
}

struct Rpc {
    next_id: u64,
}

impl Rpc {
    fn new() -> Self {
        Self { next_id: 0 }
    }

    fn call(
        &mut self,
        ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let msg = json!({ "id": id, "method": method, "params": params });
        ws.send(tungstenite::Message::Text(msg.to_string().into()))
            .map_err(|e| format!("发送调试命令失败: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let m = ws
                .read()
                .map_err(|e| format!("读取调试结果失败: {e}"))?;
            let txt = match m {
                tungstenite::Message::Text(t) => t.to_string(),
                _ => continue,
            };
            let v: Value = match serde_json::from_str(&txt) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id").and_then(|x| x.as_u64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(format!("{method} 失败: {err}"));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err(format!("{method} 超时"))
    }

    fn eval(
        &mut self,
        ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
        expr: &str,
    ) -> Result<Value, String> {
        let r = self.call(ws, "Runtime.evaluate", json!({ "expression": expr, "returnByValue": true }))?;
        let raw = r.get("result").and_then(|x| x.get("value")).cloned().unwrap_or(Value::Null);
        match raw {
            Value::String(s) => serde_json::from_str(&s).map_err(|e| format!("解析页面状态失败: {e}")),
            v => Ok(v),
        }
    }

    /// 把此刻积压在 socket 里的 Network.responseReceived 事件全部消化掉
    fn drain(
        &mut self,
        ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
        media: &mut Vec<MediaHit>,
    ) {
        set_timeout(ws.get_mut(), Some(Duration::from_millis(120)));
        loop {
            match ws.read() {
                Ok(tungstenite::Message::Text(t)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if v.get("method").and_then(|m| m.as_str()) == Some("Network.responseReceived") {
                            if let Some(resp) = v.get("params").and_then(|p| p.get("response")) {
                                if let Some(hit) = media_from_response(resp) {
                                    if !media.iter().any(|m| m.url == hit.url) {
                                        media.push(hit);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        set_timeout(ws.get_mut(), None);
    }

    #[allow(clippy::type_complexity)]
    fn cookies(
        &mut self,
        ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    ) -> Result<Vec<(String, String, String, String, bool, f64)>, String> {
        let r = self.call(ws, "Network.getAllCookies", json!({}))?;
        let arr = r.get("cookies").and_then(|c| c.as_array()).cloned().unwrap_or_default();
        Ok(arr
            .iter()
            .map(|c| {
                (
                    c.get("domain").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    c.get("path").and_then(|x| x.as_str()).unwrap_or("/").to_string(),
                    c.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    c.get("value").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    c.get("secure").and_then(|x| x.as_bool()).unwrap_or(false),
                    c.get("expires").and_then(|x| x.as_f64()).unwrap_or(0.0),
                )
            })
            .collect())
    }
}

fn set_timeout(s: &mut tungstenite::stream::MaybeTlsStream<std::net::TcpStream>, d: Option<Duration>) {
    if let tungstenite::stream::MaybeTlsStream::Plain(t) = s {
        let _ = t.set_read_timeout(d);
    }
}

fn media_from_response(resp: &Value) -> Option<MediaHit> {
    let url = resp.get("url").and_then(|u| u.as_str())?.to_string();
    let mime = resp.get("mimeType").and_then(|m| m.as_str()).unwrap_or("").to_string();
    let looks = mime.starts_with("video/")
        || mime.contains("mpegurl")
        || mime.contains("dash+xml")
        || url.contains(".mp4")
        || url.contains(".m3u8")
        || url.contains(".mpd")
        || url.contains(".webm")
        || url.contains("videoplayback");
    if !looks {
        return None;
    }
    if url.starts_with("data:") || url.starts_with("blob:") {
        return None;
    }
    let size = resp
        .get("headers")
        .and_then(|h| h.get("content-length"))
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    Some(MediaHit { url, mime, size })
}

/// 从候选媒体地址里挑一个最像「正片」的
///
/// 排序依据（依次比较）：不是广告/预览小文件 > 体积大 > 是流清单(m3u8/mpd) > 出现得晚
pub fn pick_media(hits: &[MediaHit]) -> Option<MediaHit> {
    let mut cands: Vec<&MediaHit> = hits
        .iter()
        .filter(|h| !is_junk_media(&h.url))
        .collect();
    if cands.is_empty() {
        return None;
    }
    cands.sort_by(|a, b| {
        let sa = stream_score(a);
        let sb = stream_score(b);
        sb.cmp(&sa).then(b.size.cmp(&a.size))
    });
    cands.first().map(|h| (*h).clone())
}

fn stream_score(h: &MediaHit) -> u8 {
    if h.url.contains(".m3u8") || h.mime.contains("mpegurl") {
        3
    } else if h.url.contains(".mpd") || h.mime.contains("dash+xml") {
        2
    } else if h.size > 300_000 {
        2
    } else {
        1
    }
}

fn is_junk_media(url: &str) -> bool {
    let low = url.to_lowercase();
    [
        "preview",
        "thumb",
        "/ad/",
        "advert",
        "trailer",
        "uuu_265",
        "placeholder",
        "loading.mp4",
    ]
    .iter()
    .any(|k| low.contains(k))
}

/// 只嗅媒体地址（不关心 cookie）：extractor 全挂时的最后一条路
/// FlareSolverr 代取页面的结果：挖到的媒体地址 + 页面标题（拿来做文件名）
pub struct FlarePage {
    pub hit: MediaHit,
    pub title: String,
}

/// 让 FlareSolverr 代取页面 HTML，再从 HTML 里挖媒体地址
///
/// 关键点：cf_clearance 绑定「IP + 那台客户端的 TLS 指纹」，把它的 cookie 搬给 yt-dlp 一定 403。
/// 所以这里不让它给 cookie，而是让它直接把页面取回来。
pub fn flaresolverr_media(endpoint: &str, url: &str) -> Result<FlarePage, String> {
    let base = endpoint.trim().trim_end_matches('/');
    let body = json!({ "cmd": "request.get", "url": url, "maxTimeout": 60000 }).to_string();
    let out = Command::new("curl")
        .arg("-sS")
        .arg("--max-time")
        .arg("90")
        .arg("-X")
        .arg("POST")
        .arg(format!("{base}/v1"))
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("--data-binary")
        .arg(&body)
        .output()
        .map_err(|e| format!("调用 FlareSolverr 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "FlareSolverr 没响应（{}）：确认容器在跑，地址填对了",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("FlareSolverr 返回的不是 JSON: {e}"))?;
    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status != "ok" {
        return Err(format!(
            "FlareSolverr 没解开：{}",
            v.get("message").and_then(|m| m.as_str()).unwrap_or(status)
        ));
    }
    let html = v
        .get("solution")
        .and_then(|s| s.get("response"))
        .and_then(|r| r.as_str())
        .unwrap_or("");
    let page_title = html_title(html);
    // 页面自己声明的主视频优先：一页里往往还塞了几十条推荐视频的直链，
    // 只按字典序 / 体积挑会挑到别人的片（pexels 实测挑中过 aigc-bundle 装饰视频）。
    if let Some(u) = declared_video_url(html) {
        return Ok(FlarePage {
            hit: MediaHit { mime: mime_for(&u), size: 0, url: u },
            title: page_title,
        });
    }
    let mut hits: Vec<MediaHit> = Vec::new();
    for pat in ["m3u8", "mpd", "mp4", "webm"] {
        for u in find_urls(html, pat) {
            if !hits.iter().any(|h| h.url == u) {
                hits.push(MediaHit {
                    url: u,
                    mime: format!("video/{pat}"),
                    size: 0,
                });
            }
        }
    }
    let hit = pick_media(&hits)
        .ok_or_else(|| "FlareSolverr 解开了挑战，但页面 HTML 里没有媒体地址".to_string())?;
    Ok(FlarePage {
        hit,
        title: page_title,
    })
}

/// 从 HTML 里抽 <title>，抽不到就返回空串
fn html_title(html: &str) -> String {
    // 按字节找，避免 to_lowercase 改变长度后切片落在字符中间
    let bytes = html.as_bytes();
    let open_tag = find_ci(bytes, b"<title");
    let open_tag = match open_tag {
        Some(i) => i,
        None => return String::new(),
    };
    let body = match bytes[open_tag..].iter().position(|b| *b == b'>') {
        Some(i) => open_tag + i + 1,
        None => return String::new(),
    };
    let close = match find_ci(&bytes[body..], b"</title") {
        Some(i) => body + i,
        None => return String::new(),
    };
    let raw = std::str::from_utf8(&bytes[body..close]).unwrap_or("");
    let decoded = raw
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    decoded.trim().to_string()
}

/// 大小写不敏感的子串查找（按字节）
fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let n0 = needle[0].to_ascii_lowercase();
    for i in 0..=hay.len() - needle.len() {
        if hay[i].to_ascii_lowercase() != n0 {
            continue;
        }
        let mut ok = true;
        for j in 1..needle.len() {
            if hay[i + j].to_ascii_lowercase() != needle[j].to_ascii_lowercase() {
                ok = false;
                break;
            }
        }
        if ok {
            return Some(i);
        }
    }
    None
}

/// 把 HTML 里被转义过的媒体地址还原成能直接喂给 yt-dlp 的 URL
fn normalize_media_url(raw: &str) -> String {
    let s = raw
        .replace("\\u002F", "/")
        .replace("\\u002f", "/")
        .replace("\\/", "/")
        .replace("\\u0026", "&")
        .replace("\\u003A", ":")
        .replace("\\u003a", ":");
    let low = s.to_ascii_lowercase();
    if low.contains("%3a") || low.contains("%2f") {
        percent_decode(&s)
    } else {
        s
    }
}

fn percent_decode(s: &str) -> String {
    fn hex(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 页面自己声明的「主视频」地址优先：schema.org VideoObject 的 contentUrl，
/// 其次 og:video / twitter:player。比在一堆 .mp4 里瞎挑可靠 —— 一个页面里
/// 往往还塞了几十条「推荐视频」的直链，按字典序或大小挑会挑到别人的片。
fn declared_video_url(html: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let key = b"contentUrl";
    let mut from = 0usize;
    while from < bytes.len() {
        let pos = match find_ci(&bytes[from..], key) {
            Some(p) => from + p,
            None => break,
        };
        let after_key = pos + key.len();
        // "contentUrl":"https:\u002F\u002F..." —— 依次跳过键的收尾引号、值的开引号
        let q1 = match bytes[after_key..].iter().position(|b| *b == b'"') {
            Some(i) => after_key + i,
            None => break,
        };
        let q2 = match bytes[q1 + 1..].iter().position(|b| *b == b'"') {
            Some(i) => q1 + 1 + i,
            None => break,
        };
        let q3 = match bytes[q2 + 1..].iter().position(|b| *b == b'"') {
            Some(i) => q2 + 1 + i,
            None => break,
        };
        let raw = std::str::from_utf8(&bytes[q2 + 1..q3]).unwrap_or("");
        let u = normalize_media_url(raw);
        if looks_like_media(&u) {
            return Some(u);
        }
        from = q3 + 1;
    }
    // 退一步：<meta property="og:video" content="..."> / twitter:player
    for meta_key in ["og:video", "twitter:player"] {
        if let Some(u) = meta_content(html, meta_key) {
            let u = normalize_media_url(&u);
            if looks_like_media(&u) {
                return Some(u);
            }
        }
    }
    None
}

/// 找到含指定 property/name 的 <meta> 标签里的 content="..."
fn meta_content(html: &str, key: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let kb = key.as_bytes();
    let mut from = 0usize;
    while from < bytes.len() {
        let pos = find_ci(&bytes[from..], kb)? + from;
        let tag_end = bytes[pos..]
            .iter()
            .position(|b| *b == b'>')
            .map(|i| pos + i)?;
        let tag = std::str::from_utf8(&bytes[pos..tag_end]).unwrap_or("");
        if let Some(c) = attr_value(tag, "content") {
            return Some(c);
        }
        from = tag_end + 1;
    }
    None
}

fn attr_value(tag: &str, name: &str) -> Option<String> {
    let b = tag.as_bytes();
    let nb = name.as_bytes();
    let pos = find_ci(b, nb)?;
    let rest = &b[pos + nb.len()..];
    let eq = rest.iter().position(|c| *c == b'=')?;
    let after = &rest[eq + 1..];
    let q = after.iter().position(|c| *c == b'"' || *c == b'\'')?;
    let quote = after[q];
    let value = &after[q + 1..];
    let end = value.iter().position(|c| *c == quote)?;
    std::str::from_utf8(&value[..end]).ok().map(|s| s.to_string())
}

fn looks_like_media(u: &str) -> bool {
    if !u.starts_with("http") || !u.contains("://") {
        return false;
    }
    let low = u.to_ascii_lowercase();
    [".mp4", ".m3u8", ".mpd", ".webm", ".mkv", ".flv", ".m4a", ".mp3"]
        .iter()
        .any(|e| low.contains(e))
}

fn mime_for(url: &str) -> String {
    let low = url.to_ascii_lowercase();
    if low.contains(".m3u8") || low.contains("mpegurl") {
        "application/vnd.apple.mpegurl".into()
    } else if low.contains(".mpd") {
        "application/dash+xml".into()
    } else if low.contains(".webm") {
        "video/webm".into()
    } else if low.contains(".mp3") {
        "audio/mpeg".into()
    } else {
        "video/mp4".into()
    }
}

/// 在 HTML 里挖出形如 http...ext 的地址。
///
/// 必须按字节扫：HTML 里到处是多字节字符（中文、é 等），
/// 用 `&html[i..]` 这种按字符边界的切片一旦切在字符中间就会 panic。
fn find_urls(html: &str, ext: &str) -> Vec<String> {
    let needle = format!(".{ext}");
    let nb = needle.as_bytes();
    let bytes = html.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i + nb.len() <= bytes.len() {
        if &bytes[i..i + nb.len()] == nb {
            // 往回找 url 起点：引号 / 括号 / 空白 / 标签符都算分隔
            let mut start = i;
            while start > 0 {
                let c = bytes[start - 1];
                if matches!(c, b'"' | b'\'' | b'(' | b' ' | b'\n' | b'\t' | b'<' | b'>' | b'=' | b',') {
                    break;
                }
                start -= 1;
            }
            if let Ok(raw) = std::str::from_utf8(&bytes[start..i + nb.len()]) {
                // 页面里媒体地址常被转义：JSON-LD 写成带 \u002F 的形式，
                // 下载按钮写成 file-url=https%3A%2F%2F...。不还原就是死链。
                let cand = normalize_media_url(raw);
                if cand.starts_with("http") && cand.contains("://") && cand.len() > 20 {
                    out.push(cand);
                }
            }
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

fn http_get(url: &str) -> Result<String, String> {
    let out = Command::new("curl")
        .arg("-sS")
        .arg("--max-time")
        .arg("10")
        .arg(url)
        .output()
        .map_err(|e| format!("请求调试端口失败: {e}"))?;
    if !out.status.success() {
        return Err(format!("请求调试端口失败: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[allow(dead_code)]
fn read_all(mut r: impl Read) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)?;
    Ok(buf)
}

#[allow(dead_code)]
fn write_all(mut w: impl Write, b: &[u8]) -> std::io::Result<()> {
    w.write_all(b)
}