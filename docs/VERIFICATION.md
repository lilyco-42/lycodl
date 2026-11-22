# 验证记录（2026-11-22T17:16:37+08:00）

| 项 | 命令 | 结果 |
|---|---|---|
| adapter 契约 | `node publish-driver/validate-adapters.mjs` | adapters ok: 3 (weibo, xhs, zhihu) |
| adapter 单测 | `node --test publish-driver/adapter.test.mjs` | ℹ pass 6 ℹ fail 0  |
| 扩展静态 | `node extension/validate-extension.mjs` | extension ok: v0.2.1, 6 referenced files |
| Rust 测试 | `cargo test` | test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |

## 双执行器（同一 adapter：链路 + 网页桥接）
```
extension id chmjogpmakiogbmfafgdaippdcjipdga
executor result {"status":"ok","detail":"extension executor ok","url":null}
bridge result {"status":"ok","detail":"extension executor ok","url":null}
OK: extension executor, page bridge and manifest validated
```

## 9 目的地 dry-run
```json
{"dest":"quark","status":"dry-run","detail":"演练：probe.mp4 将发布到 quark"}
{"dest":"bilibili","status":"dry-run","detail":"演练：probe.mp4 将发布到 bilibili"}
{"dest":"douyin","status":"dry-run","detail":"演练：probe.mp4 将发布到 douyin"}
{"dest":"xhs","status":"dry-run","detail":"演练：probe.mp4 将发布到 xhs"}
{"dest":"kuaishou","status":"dry-run","detail":"演练：probe.mp4 将发布到 kuaishou"}
{"dest":"youtube","status":"dry-run","detail":"演练：probe.mp4 将发布到 youtube"}
{"dest":"x","status":"dry-run","detail":"演练：probe.mp4 将发布到 x"}
{"dest":"zhihu","status":"dry-run","detail":"演练：probe.mp4 将发布到 zhihu"}
{"dest":"weibo","status":"dry-run","detail":"演练：probe.mp4 将发布到 weibo"}
```

## capabilities（6/7，quark 为已知缺口）
```json
[
 {
  "dest": "quark",
  "available": false,
  "reason": "unavailable",
  "detail": "API returned: Invalid service ticket"
 },
 {
  "dest": "bilibili",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 },
 {
  "dest": "douyin",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 },
 {
  "dest": "xhs",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 },
 {
  "dest": "kuaishou",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 },
 {
  "dest": "youtube",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 },
 {
  "dest": "x",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 },
 {
  "dest": "weibo",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 },
 {
  "dest": "zhihu",
  "available": true,
  "reason": "ready",
  "detail": "CDP 登录态可用"
 }
]
```
