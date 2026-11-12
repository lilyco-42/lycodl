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
    authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority)
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
