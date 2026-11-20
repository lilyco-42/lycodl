use serde_json::Value;
use std::time::Duration;

/// Classify a /json/version response without contacting the browser.
pub fn classify(v: &Value) -> (bool, String) {
    let ws = v
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .unwrap_or("");
    if ws.is_empty() {
        return (false, "CDP 缺少 webSocketDebuggerUrl".to_string());
    }
    let browser = v
        .get("Browser")
        .and_then(Value::as_str)
        .unwrap_or("Chrome CDP");
    (true, format!("{} 已就绪", browser))
}

/// Lightweight, read-only CDP availability check for /api/health.
pub async fn check(url: &str) -> (bool, String) {
    let base = url.trim_end_matches('/');
    if base.is_empty() {
        return (false, "未配置发布浏览器 CDP 地址".to_string());
    }
    let target = format!("{base}/json/version");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(e) => return (false, format!("CDP 客户端创建失败: {e}")),
    };
    match client.get(&target).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
            Ok(v) => classify(&v),
            Err(e) => (false, format!("CDP 响应不是 JSON: {e}")),
        },
        Ok(resp) => (false, format!("CDP HTTP {}", resp.status())),
        Err(e) => (false, format!("CDP 不可达: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_accepts_real_version_payload() {
        let v: Value = serde_json::from_str(
            r#"{"Browser":"Chrome/156.0.0.0","webSocketDebuggerUrl":"ws://127.0.0.1:9333/devtools/browser/x"}"#,
        )
        .unwrap();
        let (ready, detail) = classify(&v);
        assert!(ready);
        assert!(detail.contains("Chrome/156.0.0.0"));
    }

    #[test]
    fn classify_rejects_missing_websocket() {
        let v: Value = serde_json::from_str(r#"{"Browser":"Chrome/156.0.0.0"}"#).unwrap();
        let (ready, detail) = classify(&v);
        assert!(!ready);
        assert!(detail.contains("webSocketDebuggerUrl"));
    }
}
