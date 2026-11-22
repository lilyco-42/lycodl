# 发布适配器（adapter v1）

新增一个发布平台 = **只加一个 JSON 文件**，不改 `platform-publish.mjs`、不改 Rust、不改前端。

```
publish-driver/
  adapters/
    schema.json     # 协议定义（draft 2020-12）
    xhs.json        # 小红书
    zhihu.json      # 知乎
    weibo.json      # 微博
  adapter-runner.mjs        # Node 执行器（CDP）
  validate-adapters.mjs     # 契约校验 + 单元测试入口
extension/adapter-executor.js # 扩展执行器（chrome.debugger，免 Node）
```

## 1. 加一个平台（3 步）

1. 复制一个现成 adapter，改成新站点：

```json
{
  "id": "bilibili2",
  "name": "示例平台",
  "origin": "https://example.com",
  "cookies": [["SESSION", ".example.com"]],
  "steps": [
    { "action": "goto", "url": "https://example.com/upload" },
    { "action": "waitFor", "expr": "!!document.querySelector('input[type=file]')", "timeoutMs": 60000 },
    { "action": "setFile", "selector": "input[type=file]" },
    { "action": "fill", "selector": "input[placeholder*=标题]", "value": "$title", "maxBytes": 40 },
    { "action": "clickText", "texts": ["发布"] }
  ],
  "result": { "success": { "texts": ["发布成功"], "timeoutMs": 180000 } }
}
```

2. 校验：

```bash
node publish-driver/validate-adapters.mjs      # 契约
node --test publish-driver/adapter.test.mjs     # 单测（含全部 adapter）
```

3. 能力预筛 + 演练：

```bash
printf '{"file":"/tmp/demo.mp4","dryRun":true,"pending":["bilibili2"],"cdpUrl":"http://127.0.0.1:9333"}' \
  | node publish-driver/driver.mjs
```

`platforms.mjs` 会自动扫描 `adapters/*.json`，新目的地立即出现在 `DESTS` / capabilities / 前端列表里。

## 2. 协议字段

| 字段 | 必填 | 说明 |
|---|---|---|
| `id` | 是 | 小写字母/数字/`_`/`-`，且必须与文件名一致 |
| `name` | 是 | 展示名 |
| `origin` | 是 | `https://` 开头，用于建标签页 |
| `cookies` | 否 | 登录判定规则 `[cookieName, domain]`，只检查名与过期，不读值 |
| `maxFileBytes` | 否 | 超过即拒绝（默认按 2GB 处理） |
| `timeoutMs` | 否 | 全局预算 |
| `steps` | 是 | 动作序列，见下表 |
| `result.success` | 是 | `expr` / `texts` / `urlIncludes` 至少一个 |
| `result.urlExpr` | 否 | 成功后取作品链接的 JS 表达式 |
| `result.detail` | 否 | 成功文案 |

## 3. 动作

| action | 参数 | 作用 |
|---|---|---|
| `goto` | `url` | 打开页面（`Page.navigate`） |
| `waitFor` | `expr` / `timeoutMs` / `intervalMs` | 轮询 JS 表达式直到真值 |
| `waitMs` | `ms` | 固定等待（给前端渲染/上传留时间） |
| `setFile` | `selector` | 把本地文件塞进 `input[type=file]` |
| `fill` | `selector` / `value` / `maxBytes` | 聚焦后 `Input.insertText`，按字节截断 |
| `clickText` | `texts` / `selector` | 点文本精确匹配的可见元素（最后一个） |
| `clickDeep` | `texts` / `preferTag` | 递归 shadow DOM 查找并点击（小红书/YouTube 需要） |
| `eval` | `expr` | 任意 JS，返回值回传 |
| `press` | `key` | 键盘事件 |

每个 step 支持 `optional: true`：失败/超时继续，不算错。

## 4. 变量

`goto/fill/clickText/clickDeep/eval` 的值里可用：

| 变量 | 来源 |
|---|---|
| `$file` | 本地视频绝对路径 |
| `$fileName` | 文件名 |
| `$title` | 标题（`fill` 的 `maxBytes` 负责平台截断） |
| `$desc` | 标题 + 网盘链接 + 提取码拼好的描述 |
| `$shareUrl` / `$sharePasscode` | 夸克分享 |
| `$visibility` | `public` / `private` |

## 5. 两个执行器，同一份 adapter

| 执行器 | 通道 | 依赖 | 适用 |
|---|---|---|---|
| Node（默认） | CDP `ws://127.0.0.1:9333` | Node 18+ | 已有环境、可脚本化 |
| 扩展 | `chrome.debugger`（等价 CDP） | 只要 Chrome | **Windows 免 Node** |

扩展执行器需要用户在首次调用时同意一次「调试浏览器」授权；adapter 由 `GET /api/publish/adapters/:id` 提供，扩展只负责执行。

强制走 adapter（用于等价验证）：

```bash
LYCODL_ADAPTER_FIRST=1 printf '...' | node publish-driver/platform-publish.mjs
```

默认仍走各平台原有的手写流程，保证 7 个老目的地零回归。

## 6. 已知限制

- adapter 是「步骤描述」，DOM 变更仍会失效；平台改版时要按第 1 步重新校准。
- 知乎/微博的 adapter 是首版（入口 + 常见选择器），必须在登录态下真机校准一次才算可用。
- 扩展执行器无法绕过平台风控；小红书等站点仍可能要求 App 扫码。

## 7. 测试

- `node publish-driver/validate-adapters.mjs`：契约（id/文件名/origin/动作参数/成功判定）
- `node --test publish-driver/adapter.test.mjs`：6 项，含「全部 adapter 过契约」「schema 拒绝坏输入」
- `cargo test`：Rust 侧不感知 adapter 内容，只保证 API 路由与台账不回退