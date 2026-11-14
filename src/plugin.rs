//! 浏览器扩展通道：本地令牌 + 窄白名单 CORS + Private Network Access 预检。
//!
//! 为什么不能简单开一个 Access-Control-Allow-Origin: *？
//! 本机服务原本没有鉴权，一旦放行任意网页，任何页面都能偷偷往下载器塞任务。
//! 所以：只有 chrome-extension:// / moz-extension:// 能跨域，
//! 且必须带存放在 data_dir/plugin.token 的令牌；同源 Web UI 与 curl 免令牌。

use std::sync::{Arc, RwLock};

use axum::extract::{Request, State};
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

pub const TOKEN_HEADER: &str = "x-lycodl-token";
const ALLOW_METHODS: &str = "GET, POST, PUT, OPTIONS";
const DEFAULT_ALLOW_HEADERS: &str = "content-type, x-lycodl-token";
const ALLOW_PRIVATE_NETWORK: &str = "access-control-allow-private-network";

pub type SharedToken = Arc<RwLock<String>>;

fn token_path() -> std::path::PathBuf {
    crate::paths::data_dir().join("plugin.token")
}

/// 读回令牌；不存在 / 损坏就生成一份并落盘（0600）。
pub fn load_or_create_token() -> String {
    let p = token_path();
    if let Ok(s) = std::fs::read_to_string(&p) {
        let t = s.trim();
        if t.len() >= 32 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return t.to_string();
        }
    }
    let t = random_hex(32);
    let _ = persist(&t);
    t
}

/// 轮换令牌：写盘成功才让 api.rs 更新内存那份。
pub fn rotate_token() -> Result<String, String> {
    let t = random_hex(32);
    persist(&t)?;
    Ok(t)
}

fn persist(t: &str) -> Result<(), String> {
    let p = token_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
    }
    std::fs::write(&p, t).map_err(|e| format!("写插件令牌失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    let filled = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut buf))
        .is_ok();
    if !filled {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos().to_le_bytes())
                .unwrap_or_default(),
        );
        h.update(std::process::id().to_le_bytes());
        let d = h.finalize();
        let k = n.min(d.len());
        buf[..k].copy_from_slice(&d[..k]);
    }
    let mut s = String::with_capacity(n * 2);
    for b in buf {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn is_extension_origin(origin: &str) -> bool {
    origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://")
}

/// 取 authority 的 host 部分（去掉端口）。
fn host_of(authority: &str) -> &str {
    authority
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(authority)
}

fn is_local(authority: &str) -> bool {
    matches!(host_of(authority), "localhost" | "127.0.0.1" | "[::1]")
}

/// 同源判定：Host 必须是回环（挡 DNS rebinding），Origin 也必须是回环。
/// 允许端口不同是为了 Vite dev server 之类的本机开发页。
fn is_same_origin(origin: &str, host: &str) -> bool {
    if !is_local(host) {
        return false;
    }
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let authority = rest.split('/').next().unwrap_or("");
    authority.eq_ignore_ascii_case(host) || is_local(authority)
}

/// 定长比较，避免用普通 == 泄漏令牌前缀。
fn token_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for i in 0..a.len() {
        d |= a[i] ^ b[i];
    }
    d == 0
}

fn cors_headers(headers: &mut axum::http::HeaderMap, origin: &str, allow_headers: Option<&str>) {
    if let Ok(v) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
    }
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(ALLOW_METHODS),
    );
    if let Some(h) = allow_headers {
        if let Ok(v) = HeaderValue::from_str(h) {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, v);
        }
    }
    headers.insert(
        HeaderName::from_static(ALLOW_PRIVATE_NETWORK),
        HeaderValue::from_static("true"),
    );
}

fn error(status: StatusCode, msg: &str, origin: &str) -> Response {
    let mut resp = (status, Json(json!({ "error": msg }))).into_response();
    cors_headers(resp.headers_mut(), origin, Some(DEFAULT_ALLOW_HEADERS));
    resp
}

/// 同源请求免令牌；扩展请求必须带令牌；其它跨域一律拒。
/// OPTIONS 预检按规范不带自定义头，所以不校验令牌，但要回 CORS + PNA 头。
pub async fn guard(State(token): State<SharedToken>, req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // 同源 Web UI / curl / SSE：浏览器不会带 Origin，无需任何令牌
    let Some(origin) = origin else {
        return next.run(req).await;
    };

    let ext = is_extension_origin(&origin);
    let same = is_same_origin(&origin, &host);
    if !ext && !same {
        return error(
            StatusCode::FORBIDDEN,
            "跨域请求被拒绝：只允许 lycodl 页面或已安装的浏览器扩展",
            &origin,
        );
    }

    if req.method() == Method::OPTIONS {
        let asked = req
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(DEFAULT_ALLOW_HEADERS);
        let mut resp = StatusCode::NO_CONTENT.into_response();
        cors_headers(resp.headers_mut(), &origin, Some(asked));
        resp.headers_mut().insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
        return resp;
    }

    if ext {
        // 令牌本身只能在本机设置页读/换，扩展拿着令牌也不能自换
        if req.uri().path() == "/api/plugin-token" {
            return error(
                StatusCode::FORBIDDEN,
                "插件令牌只能在 lycodl 设置页查看或重新生成",
                &origin,
            );
        }
        if !req.uri().path().starts_with("/api/") {
            return error(StatusCode::FORBIDDEN, "扩展只能访问 /api/* 接口", &origin);
        }
        let supplied = req
            .headers()
            .get(TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let expected = token.read().map(|s| s.clone()).unwrap_or_default();
        if supplied.is_empty() || !token_eq(supplied, &expected) {
            return error(
                StatusCode::UNAUTHORIZED,
                "插件令牌无效或已过期：请在 lycodl 设置页重新复制令牌",
                &origin,
            );
        }
    }

    let mut resp = next.run(req).await;
    cors_headers(resp.headers_mut(), &origin, Some(DEFAULT_ALLOW_HEADERS));
    resp
}

// ── 单元测试 ───────────────────────────────────────────────────────────────
// 这个 crate 只有 bin target（没有 lib.rs），所以测试必须写在 crate 内部。
// 覆盖：纯函数 + 令牌落盘 + 真实 guard 中间件的 HTTP 行为（oneshot，无外网）。

#[cfg(test)]
mod tests {
    use super::*; // token_eq / is_* / random_hex / guard / 常量 / Request / header / StatusCode / Method / State
    use axum::body::Body;
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use axum::Router;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use tower::util::ServiceExt as _; // .oneshot(...)

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    // LYCODL_DATA_DIR 是进程级环境变量：三个落盘测试必须串行，
    // 否则其中一个会读到别人的临时目录，甚至目录已被删掉。
    static DATA_DIR_LOCK: Mutex<()> = Mutex::new(());

    /// 每个测试独占一个数据目录，避免并行测试互相踩 / 改到真实 HOME。
    fn unique_data_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "lycodl-test-{}-{}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("create temp data dir");
        d
    }

    /// 返回一把进程级锁；持锁期间独占 LYCODL_DATA_DIR。
    fn data_dir_lock() -> std::sync::MutexGuard<'static, ()> {
        DATA_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ── 纯函数 ──

    #[test]
    fn token_eq_is_length_and_content_sensitive() {
        let t = "a".repeat(64);
        assert!(token_eq(&t, &t));
        assert!(!token_eq("abc", "abcd")); // 长度不同
        let mut wrong = t.clone();
        wrong.replace_range(63..64, "b");
        assert!(!token_eq(&t, &wrong)); // 末位不同
                                        // 两个空串被判定为相等（调用方在 guard 里先用 is_empty 短路，不会真走到这里）
    }

    #[test]
    fn extension_origin_only_accepts_browser_schemes() {
        assert!(is_extension_origin("chrome-extension://abcdefghijklmnop"));
        assert!(is_extension_origin("moz-extension://1234-5678"));
        assert!(!is_extension_origin("https://evil.example"));
        assert!(!is_extension_origin("http://127.0.0.1:8766"));
        assert!(is_extension_origin("chrome-extension://")); // 裸前缀也算匹配；但 guard 里仍需有效令牌，无安全影响
        assert!(!is_extension_origin("CHROME-EXTENSION://abc")); // 大小写敏感，按规范 scheme 小写
    }

    #[test]
    fn host_of_strips_port_and_keeps_ipv6_brackets() {
        assert_eq!(host_of("127.0.0.1:8766"), "127.0.0.1");
        assert_eq!(host_of("localhost"), "localhost");
        // rsplit_once(':') 对 "[::1]:8766" 会得到 "[::1]"（方括号保留），is_local 认得
        assert_eq!(host_of("[::1]:8766"), "[::1]");
    }

    #[test]
    fn same_origin_rejects_dns_rebinding_and_remote_hosts() {
        // 回环 Host + 回环 Origin（含端口不同）→ 放行
        assert!(is_same_origin("http://127.0.0.1:5173", "127.0.0.1:8766"));
        assert!(is_same_origin("http://localhost:8766", "127.0.0.1:8766"));
        assert!(is_same_origin("http://[::1]:8766", "[::1]:8766"));
        // Host 不是回环 → 一律拒（DNS rebinding 把恶意域名指到本机）
        assert!(!is_same_origin(
            "http://127.0.0.1:8766",
            "evil.example:8766"
        ));
        assert!(!is_same_origin("http://127.0.0.1:8766", "192.168.1.5:8766"));
        // Origin 是远程站点
        assert!(!is_same_origin("https://evil.example", "127.0.0.1:8766"));
        // 非 http(s) scheme
        assert!(!is_same_origin("file:///etc/passwd", "127.0.0.1:8766"));
    }

    #[test]
    fn random_hex_is_correct_length_and_charset() {
        let s = random_hex(32);
        assert_eq!(s.len(), 64);
        assert!(s.bytes().all(|b| b.is_ascii_hexdigit()));
        let s2 = random_hex(32);
        assert_ne!(s, s2); // 两次不同（极低概率碰撞）
    }

    // ── 令牌落盘 / 轮换 ──

    #[test]
    fn token_is_created_persisted_and_reloaded() {
        let _guard = data_dir_lock();
        let dir = unique_data_dir();
        std::env::set_var("LYCODL_DATA_DIR", &dir);
        let t = load_or_create_token();
        assert_eq!(t.len(), 64);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        let f = dir.join("plugin.token");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), t);
        // 再读一次应复用，不重新生成
        assert_eq!(load_or_create_token(), t);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "plugin.token 必须是 0600");
        }
        std::env::remove_var("LYCODL_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotate_token_changes_disk_and_memory_value() {
        let _guard = data_dir_lock();
        let dir = unique_data_dir();
        std::env::set_var("LYCODL_DATA_DIR", &dir);
        let old = load_or_create_token();
        let new = rotate_token().expect("rotate");
        assert_ne!(old, new);
        assert_eq!(new.len(), 64);
        assert_eq!(
            std::fs::read_to_string(dir.join("plugin.token")).unwrap(),
            new
        );
        std::env::remove_var("LYCODL_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_token_is_regenerated() {
        let _guard = data_dir_lock();
        let dir = unique_data_dir();
        std::env::set_var("LYCODL_DATA_DIR", &dir);
        std::fs::write(dir.join("plugin.token"), "garbage-not-hex").unwrap();
        let t = load_or_create_token();
        assert_eq!(t.len(), 64);
        assert!(t.bytes().all(|b| b.is_ascii_hexdigit()));
        std::env::remove_var("LYCODL_DATA_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 真实 guard 中间件的 HTTP 行为 ──

    /// 用真实 guard 包一层最小路由；内层只回 200 "ok"，这样状态码完全由 guard 决定。
    fn test_app() -> (Router, SharedToken, String) {
        let token: SharedToken = Arc::new(RwLock::new("0123456789abcdef".repeat(4)));
        let app: Router = Router::new()
            .route("/api/health", get(|| async { "ok" }))
            .route("/api/plugin-token", get(|| async { "secret-page" }))
            .route("/index.html", get(|| async { "page" }))
            .layer(from_fn_with_state(token.clone(), guard));
        let expected = token.read().unwrap().clone();
        (app, token, expected)
    }

    fn req(
        method: axum::http::Method,
        path: &str,
        origin: Option<&str>,
        host: &str,
        token_hdr: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Request<Body> {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, host);
        if let Some(o) = origin {
            b = b.header(header::ORIGIN, o);
        }
        if let Some(t) = token_hdr {
            b = b.header(TOKEN_HEADER, t);
        }
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        b.body(Body::empty()).unwrap()
    }

    fn has_pna(r: &axum::response::Response) -> bool {
        r.headers()
            .get(ALLOW_PRIVATE_NETWORK)
            .and_then(|v| v.to_str().ok())
            == Some("true")
    }

    #[tokio::test]
    async fn no_origin_is_same_site_and_needs_no_token() {
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/api/health",
                None,
                "127.0.0.1:8766",
                None,
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }

    #[tokio::test]
    async fn extension_without_token_is_401() {
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/api/health",
                Some("chrome-extension://abcdefghijklmnop"),
                "127.0.0.1:8766",
                None,
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        assert!(has_pna(&r)); // 错误响应也带 CORS/PNA
        assert_eq!(
            r.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "chrome-extension://abcdefghijklmnop"
        );
    }

    #[tokio::test]
    async fn extension_with_wrong_token_is_401() {
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/api/health",
                Some("chrome-extension://abcdefghijklmnop"),
                "127.0.0.1:8766",
                Some("deadbeef"),
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn extension_with_valid_token_gets_full_cors() {
        let (app, _, tok) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/api/health",
                Some("moz-extension://1234-5678"),
                "127.0.0.1:8766",
                Some(&tok),
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            r.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "moz-extension://1234-5678"
        );
        assert!(has_pna(&r));
        assert_eq!(r.headers().get(header::VARY).unwrap(), "Origin");
        assert_eq!(
            r.headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .unwrap(),
            ALLOW_METHODS
        );
    }

    #[tokio::test]
    async fn extension_cannot_rotate_the_token_itself() {
        let (app, _, tok) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/api/plugin-token",
                Some("chrome-extension://abcdefghijklmnop"),
                "127.0.0.1:8766",
                Some(&tok),
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn extension_cannot_reach_non_api_paths() {
        let (app, _, tok) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/index.html",
                Some("chrome-extension://abcdefghijklmnop"),
                "127.0.0.1:8766",
                Some(&tok),
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn malicious_web_origin_is_403() {
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::POST,
                "/api/download",
                Some("https://evil.example"),
                "127.0.0.1:8766",
                None,
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn same_origin_needs_no_token() {
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/api/health",
                Some("http://127.0.0.1:8766"),
                "127.0.0.1:8766",
                None,
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn dns_rebinding_host_is_rejected() {
        // Host 是外部域名（rebinding），即便 Origin 写成回环也拒
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::GET,
                "/api/health",
                Some("http://127.0.0.1:8766"),
                "attacker.example:8766",
                None,
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn options_preflight_is_204_with_pna_and_no_token_needed() {
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::OPTIONS,
                "/api/download",
                Some("chrome-extension://abcdefghijklmnop"),
                "127.0.0.1:8766",
                None, // 预检按规范不带自定义头
                &[
                    (header::ACCESS_CONTROL_REQUEST_METHOD.as_str(), "POST"),
                    (
                        header::ACCESS_CONTROL_REQUEST_HEADERS.as_str(),
                        "content-type, x-lycodl-token",
                    ),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            r.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "chrome-extension://abcdefghijklmnop"
        );
        assert!(
            has_pna(&r),
            "必须回 Access-Control-Allow-Private-Network: true"
        );
        assert_eq!(
            r.headers()
                .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
                .unwrap(),
            "content-type, x-lycodl-token"
        );
        assert_eq!(
            r.headers().get(header::ACCESS_CONTROL_MAX_AGE).unwrap(),
            "600"
        );
    }

    #[tokio::test]
    async fn options_from_malicious_origin_is_403() {
        let (app, _, _) = test_app();
        let r = app
            .oneshot(req(
                axum::http::Method::OPTIONS,
                "/api/download",
                Some("https://evil.example"),
                "127.0.0.1:8766",
                None,
                &[],
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }
}
