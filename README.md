# lycodl

多平台视频下载器。内核是独立的 **yt-dlp** 二进制，命令面由 **lilyco** 框架生成，界面是 **React + shadcn/ui** 的本地 Web 控制台。

一句话：**换站点、换签名算法都不用重新编译我 —— 热更新内核就行。**

## 为什么这样设计

YouTube 现在对 web 客户端只发 SABR 流，动不动就是 403 / "Sign in to confirm you're not a bot"。这类对抗是**持续进行**的：yt-dlp 每隔几天就发新版修一次。

所以本项目的关键决策是：**把 yt-dlp 放在进程外**。

- 内核（yt-dlp 二进制）托管在 `~/.local/share/lycodl/bin/yt-dlp`，与程序本体解耦；
- `lycodl kernel-update` 或设置页的「立即更新」从 GitHub Releases 拉最新版，校验 SHA-256、验证能跑、原子替换；
- 替换后**不需要重启服务**，下一个任务立刻用新内核 —— 这就是「吃更新红利」。

## 四端同源

同一份 `lilyco` 命令注册表驱动四种入口，命令签名即 AI 可直接调用的工具签名：

```bash
lycodl --version    # 本体版本 + 内核版本（排查用）
lycodl              # 裸跑 → 起本地 Web 控制台（主入口，自动开浏览器）
lycodl --web        # 同上，显式
lycodl <子命令>     # CLI，脚本友好
lycodl --tui        # 终端 UI
lycodl --mcp        # MCP 服务器（stdio JSON-RPC），交给 Agent 调用
```

## 安装 / 运行

```bash
cd web && pnpm install && pnpm build   # 前端产物会被 include_dir 编进二进制
cd .. && cargo build --release
./target/release/lycodl
```

首次运行会自动探测系统里已有的 yt-dlp；想用托管副本就执行一次 `lycodl kernel-update`。

## 常驻与单实例

重复执行 `lycodl --web` 不会叠出第二个服务：程序会把端口记在 `~/.local/share/lycodl/web.port`，启动前先探一下那个端口上的 `/api/health` 是不是自己（校验 `name == "lycodl"`，不认错别的程序），是就直接打印

```
lycodl 已在运行: http://127.0.0.1:8766
```

然后退出。想开机自启，用 systemd user 单元（本仓库不带安装脚本，手写即可）：

```ini
# ~/.config/systemd/user/lycodl.service
[Unit]
Description=lycodl 视频下载器

[Service]
Type=simple
Environment=LYCODL_NO_OPEN=1
ExecStart=%h/.local/bin/lycodl --web
Restart=on-failure
RestartSec=3
TimeoutStopSec=20

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now lycodl.service
```

## 命令

| 命令 | 分级 | 说明 |
| --- | --- | --- |
| `probe --url <url>` | T0 只读 | 解析链接，列出可用格式（不下载） |
| `formats --url <url>` | T0 只读 | 人类可读的格式表 |
| `files` | T0 只读 | 列出输出目录里的成品文件 |
| `kernel` | T0 只读 | 当前内核路径 / 版本 / 来源（managed 还是 system） |
| `settings` | T0 只读 | 查看当前配置 |
| `download --url <url>` | T1 需确认 | 下载，进度实时上报 |
| `kernel-update` | T1 需确认 | 从 GitHub 拉最新 yt-dlp 覆盖托管副本（热更新） |
| `settings-set` | T1 需确认 | 修改配置 |

所有参数都是显式长选项（lilyco 生成的命令面统一用 `--name` 形式，没有位置参数），例如：

```bash
lycodl formats  --url https://www.youtube.com/watch?v=jNQXAC9IVRw
lycodl download --url https://www.bilibili.com/video/BV1GJ411x7h7 --mode audio
lycodl download --url <视频页> --mode custom --format-id 137
```

`lycodl --schema` 输出全部命令的 JSON Schema，`--help` 看人类可读版本；每个子命令还支持 `--json` / `--openai-tool` / `--anthropic-tool` / `--gemini-tool` 等机器可读输出，方便直接塞给 LLM 当工具定义。

内核巡检是后台做的：服务启动时以及之后每 6 小时问一次 GitHub，把最新版本号缓存在内存里，所以设置页一打开就能看到「最新版本」，不用手点「检查更新」。更新是**幂等**的 —— 已经是最新版就直接返回 `alreadyLatest: true`，不会白拉 41 MB；要强制重装设 `LYCODL_KERNEL_FORCE=1`。

## 网页控制台

六个页签：**下载**（粘贴链接 → 解析 → 选模式/画质 → 提交）、**任务**（SSE 实时进度、取消、重试、日志）、**文件**（成品列表、直接下载、定位到文件夹）、**设置**（输出目录、浏览器 Cookie、代理、并发数、指纹伪装、直链兜底、内核更新）、**平台**（运行环境自检、主流站点目录、提取器搜索）、**命令（AI）**（把注册表当工具签名展示，可复制 CLI 用法给 AI）。

「定位」走的是 `POST /api/reveal`：后端用 D-Bus 的 `org.freedesktop.FileManager1.ShowItems` 让 Dolphin / Nautilus 直接**选中**那个文件（实测拉起 `dolphin --select <路径>`），拉不起来就退一步 `xdg-open` 打开所在目录，再不行前端退回「复制路径到剪贴板」。该接口只允许定位输出目录内的路径，不能拿来当任意文件的打开原语。

前端构建产物通过 `include_dir!` 编进二进制，单个可执行文件即可运行（health 里的 `webMode` 会显示 `embedded`）。

## 全平台支持

yt-dlp 认识 1398 个站点提取器（`--list-extractors` 实测 1755 行，其中 136 个上游自己标注了 CURRENTLY BROKEN）。所以「支持多少平台」不是靠本项目写死名单，而是靠三件事：

**一、把前置条件摊开给人看。** `GET /api/caps`（平台页顶部「运行环境自检」）一次查完：内核版本与来源、JS 运行时、ffmpeg、指纹伪装目标、本机有哪些浏览器可当 Cookie 源、提取器覆盖量、磁盘余量。任何一项缺失都会在 `warnings` 里给出「缺了会怎样 + 去哪补」。这些正是「为什么这个站下不了」的全部原因，不用再一个个猜。

**二、解析不再硬扛，而是逐级降级。** `POST /api/probe` 的 `strategy` 字段会说清楚它到底试了什么：

| 策略 | 什么时候触发 | 说明 |
| --- | --- | --- |
| `default` | 总是第一个 | 就用设置里的 Cookie / 代理 / 指纹伪装 |
| `cookies` | 设置里没选 Cookie 时 | 自动挑一个本机装了的浏览器取 Cookie |
| `impersonate` | 设置里没开伪装、且内核报有可用目标 | TLS 指纹伪装，对付 Cloudflare 一类的 403 |
| `impersonate+cookies` | 上一条失败 | 两者叠加 |
| `youtube-client` | 链接是 YouTube | `--extractor-args youtube:player_client=web_safari` |

同一站点最多试 4 次（YouTube 因为多一条备用播放客户端，计划里会排到 5 条，但同样封顶 4 次）。**成功的策略会按站点记进 `~/.config/lycodl/strategies.json`**，下次同站直接先走那条路 —— 平台页的卡片上会显示「已记录策略：自动取本机浏览器 Cookie」。如果错误本身跟环境无关（`Unsupported URL`、`404`、直播已结束），立刻停下，不做无意义的重复尝试。

需要说清楚的一点：**指纹伪装不是万能药**。实测对 dailymotion / vimeo / 小红书 / 微博 / reddit / 抖音这六个疑难站点，开不开伪装结果完全一致（都是站点侧的登录 / 风控要求）。它只对「Cloudflare 前置拦截」这类场景有用，所以默认关着、只当兜底，不默认开启。更硬的反爬（Cloudflare JS challenge、需要真实浏览器渲染的页面、上游提取器本身坏掉）走下面 [反爬与直链兜底](#反爬与直链兜底)。

**三、失败信息直接给下一步动作。** `friendly_error` 现在覆盖 20 类上游报错，每一条都翻译成中文并指明操作：`Fresh cookies are needed`（选个最近开过该站的浏览器）、`Account authentication is required`（要已登录的 Cookie）、`geo restriction`（填代理）、`Unable to extract`（内核要更新）、`No video formats found`（三种可能原因分别列出）……解析全失败时还会附上「已尝试 4 种方式：默认配置（失败）→ 自动取本机浏览器 Cookie（失败）→ …」，一眼能看出卡在哪一步。

前端把这些原样暴露：解析成功后卡片上会显示站点名徽章、平台状态（可直接用 / 需 Cookie / 需登录 / 不保证）、以及可展开的「试了 N 种方式」明细；失败时错误提示条里就带着上面那段完整说明。

实测覆盖（本机 2026-11-11，全部走真实网络）：

- **无 Cookie 直接可用**：哔哩哔哩（含番剧）、TikTok、Internet Archive、PeerTube、SoundCloud、niconico、VK、Dailymotion、AcFun、微博、知乎、网易云音乐、QQ 音乐、喜马拉雅、Bandcamp、Twitch 点播、BitChute、Odysee、TED、Streamable、Instagram reel（公开内容）。
- **需要 Cookie**：YouTube（降级链自动补上本机 Chromium Cookie，实测 18 个格式）、抖音、小红书、西瓜视频。
- **需要登录**：Facebook、Vimeo、Reddit。
- **上游本身限制**（不是本项目 bug）：优酷错误码 6004、爱奇艺 `Can't find any video`、酷我 `Unable to extract song name`、虎牙直播离线、斗鱼直播源 404、快手根本没有对应提取器。

站点目录（`GET /api/sites`，36 个主流站点）里每个站点都带 `status` 与中文 `tips`，告诉你它属于上面哪一档、以及要配什么才能下。

## 反爬与直链兜底

把话说在前面：没有哪个开关能「伪装浏览器」后通杀所有站点。实测里我们否掉了几条看似合理的路：

- **把 FlareSolverr 解出的 `cf_clearance` 喂给 yt-dlp / `curl` / `--impersonate` 全部仍是 403** —— Cloudflare 的放行 Cookie 绑定「IP + 该客户端 TLS/JA3 指纹」，换个客户端就不认。拿 Cookie 没用，得让它**代取页面**；
- 本机 Chromium 无论有头（`DISPLAY=:0`）还是无头，都过不了 pexels / spankbang 的 CF JS challenge（20 秒后标题仍是「Just a moment...」）；
- CDP Cookie 桥对抖音 / 小红书 / reddit / vimeo 无改善 —— 前两个是 yt-dlp 上游提取器缺陷（[#9667](https://github.com/yt-dlp/yt-dlp/issues/9667) / [#16519](https://github.com/yt-dlp/yt-dlp/issues/16519)），后两个是真·登录墙；
- `--impersonate chrome` 对上述六个疑难站点与不开的结果完全一致，它只该在 403 时兜底，不能默认开启。
- **PO Token provider（bgutil）也救不了 bot check** —— 本机起 HTTP provider（`/get_pot` 能返回 `poToken`）、装好 yt-dlp 插件后，不带 Cookie 解析 YouTube 仍是 `Sign in to confirm you’re not a bot`；POT 是给已通过身份校验的会话补令牌，不是绕过身份校验本身。所以没把它做进降级链。

所以正确解法是**绕开解析失败的提取器，直接拿到媒体直链**，再交给 yt-dlp 当普通文件下。两条可选通道（设置页「直链兜底」，默认都关，只在前面几条常规策略全失败后才试）：

| 通道 | 怎么开 | 干什么 | 适用 |
| --- | --- | --- | --- |
| 浏览器桥 | 打开「浏览器桥」开关 | 起一个临时无头 Chromium（独立 profile / CDP），真开页面、静音播放、抓 `Network.responseReceived` 里的 m3u8 / mpd / mp4，同时导出同 UA 的 Netscape Cookie 再回喂 yt-dlp | 页面能渲染、但 yt-dlp 认不出流；或站点只认「刚访问过页面」的新鲜 Cookie |
| FlareSolverr | 填外部服务地址（默认关闭；本机容器一般填 `http://127.0.0.1:8191`） | POST `/v1` 让它解完 CF 把**页面 HTML**交回来，从 JSON-LD `VideoObject.contentUrl` / `og:video` / `twitter:player` 优先挖直链，退而扫 HTML 里的媒体 URL | Cloudflare 前置拦截、本机浏览器过不去的站 |

两者都会把 HTML 里的转义（Unicode 斜杠、反斜杠转义斜杠、百分号编码）还原成正常 URL，并且**只认页面自己声明的视频**：pexels 页面里有 60 多条 `.mp4`（推荐位、装饰视频），靠猜打分一定挑错，实测优先取 JSON-LD 的 `contentUrl` 才拿到 `https://videos.pexels.com/video-files/855029/855029-hd_1920_1080_30fps.mp4`。浏览器桥还会滤掉 `uuu_265.mp4` 一类的装饰/预览片段，抓不到目标时就明确告诉你「页面已打开（标题…），抓到 N 个媒体请求，但都像是装饰或预览片段」，不含糊装成功。

命中直链后，解析结果里多一个 `direct` 对象（`url` / `referer` / `userAgent` / `source`），预览卡上出现「直链兜底」徽章和来源说明；提交下载时前端把 `direct` 与 `directTitle` 原样回传，后端拼接 `--referer` / `--user-agent` 与输出模板 `-o「<标题> [direct].%(ext)s」` 直接把直链交给 yt-dlp（跳过 `-f` 选择，音频模式仍走 `-x`）。CLI 与 MCP 的 `probe --json` 顶层也带 `direct`，字段是附加的，旧调用方不受影响。

**端到端实证**（本机 2026-11-11）：pexels 视频页 `https://www.pexels.com/video/a-cat-855029/`，yt-dlp 直连 403 → 降级链试 default / cookies / impersonate 全败 → FlareSolverr 代取 HTML 命中 `contentUrl` → 下载任务 `done 100.0`，落盘 `Video Of Funny Cat … [direct].mp4` 13,113,884 B，ffprobe 确认 h264 / 1920×1080 / 23.42s。同一条链接在 CLI 与 MCP 的 `probe` 里也能拿到 `direct`。

**边界要说清楚**：FlareSolverr 是外部服务，得自己跑（下面给了一行 docker 命令）；浏览器桥每次失败解析会多花十几秒起浏览器；两者都只解决「能拿到直链」的问题，**不解决账号 / 地区 / 会员限制**，也不把 `cf_clearance` 伪装成万能通行证。直链是页面当时下发的地址，过期就重新解析一次。

```bash
# 可选：起 FlareSolverr（只在设置了地址时才会被用到）
docker run -d --name flaresolverr -p 8191:8191 ghcr.io/flaresolverr/flaresolverr:latest
# 然后在设置页「FlareSolverr 地址」填 http://127.0.0.1:8191
```

**收尾状态**：调研用的 bgutil POT provider 容器、yt-dlp 插件、实验用 chromium 进程均已移除；`fs-test` FlareSolverr 容器也在回归后停掉，正式服务的 `flaresolverrEndpoint` 保持 `null`（默认关闭）。需要时按上面的 docker 命令自己起一个，再在设置页填地址即可。

## 播放列表

粘贴播放列表链接（例如 `https://www.youtube.com/playlist?list=...`）解析时，解析卡片会显示「共 N 个条目」和条目清单；下方「下载整个播放列表」开关默认打开。关掉它只下当前这一条。

批量下载时进度条显示的是**整体进度**，不是当前这一项的进度：

- 后端从 `--progress-template` 里带出 `%(info.playlist_index)s`，从 `--print before_dl:` 里带出 `%(playlist_index)s` / `%(n_entries)s`；
- 整体百分比 = `(当前项序号 - 1 + 当前项进度) / 总条目数`，所以换项时不会从 100% 掉回 0%；
- 播放列表这条路径上百分比单调不回退（取 max），因为 yt-dlp 换项瞬间会吐出 `total=NA` 的残留进度行，不夹住就会看到进度往回跳；
- 任务卡片上会多一个「第 3 / 182 项」徽章，标题也跟着换成正在下哪一条。

单视频时这些模板字段都是字面量 `NA`，解析成 `None`，走原来的整条进度逻辑。

## 取消任务

`POST /api/jobs/{id}/cancel` 置取消标志，下载线程以 200ms 为周期轮询（读 stdout 走独立线程，播放列表换项时 yt-dlp 会安静几十秒，不能阻塞在读管道上）。

真正要注意的是**怎么杀**：yt-dlp 官方 linux 包是 PyInstaller 单文件，启动后父进程会再 fork 出一个真身。只 `kill` 父进程的话，真身会变成孤儿继续下载，而且还握着 stdout 管道，读线程永远等不到 EOF —— 任务就卡在 running 上，返回的 `{ok:true}` 看起来像在撒谎。

所以 spawn 时用 `libc::setsid()` 让内核进程独立成组，取消时按**进程组**发 `SIGKILL`（`kill(-pid)`）。实测播放列表任务取消后 544ms 内落定为 `cancelled`，`pgrep -xc yt-dlp` 归零。

被 SIGKILL 的 yt-dlp 来不及自己清理，会留下 `*.f397.mp4.part` 之类的半成品。所以下载开始时会用 `--print before_dl:"LYCOBASE %(filename)s"` 记下这次的目标基名，取消成功后按这个前缀清掉 `.part` / `.ytdl` / `.temp` / `.frag` 文件（视频轨、音频轨都点得到）。**只有取消才清**——非取消的失败留着半成品，下次下同一条还能续传。


## 并发与失败项

并发上限在设置页调。**调小会立刻生效**：这里的实现没有用 `Semaphore` —— 令牌一旦发出去就收不回来，那样「把并发数调小」只能等现有任务自然结束才生效，看起来就像没反应。现在用的是一个原子计数 `running` + 抢占循环（满了就 150ms 再看一眼），所以：

- 调小不会打断正在跑的任务（它们有释放在跑的槽位），只是新任务按新上限排队；
- 调大立刻补位，排队里的任务会在几百毫秒内起来；
- `PUT /api/settings` 会回报 `running` 与 `effectiveConcurrency`，设置页在「调小但仍有任务在跑」时会提示一句「当前仍在跑 N 个任务，它们不会被打断」。

播放列表是失败率最高的场景：182 项里有一项是私有 / 已删除 / 地区限制，整批就断了。所以播放列表下载默认走**跳过失败项**：

- 固定带 `--retries 10 --fragment-retries 10 --retry-sleep 2`，网络抖动自己扛；
- 播放列表额外加 `--ignore-errors`，单项失败不中断整批；
- 只要**还有成品**，非零退出码也判为成功（`status=done`），把 stderr 里的 `ERROR:` 行经 `friendly_error()` 翻成中文提示，作为 `warnings` 挂在任务上；
- 退出码为 0 时也会扫一遍 stderr —— `--ignore-errors` 让 yt-dlp 即便有项失败也返回 0，不能让「跳过」悄无声息；
- 任务卡片上会多一块琥珀色的「有 N 条提示」，列出被跳过的条目（最多显示 3 条，其余折叠）；`warnings` 上限 50 条，防止上千项的播放列表把内存撑起来。

实测（打桩内核模拟 3 项里第 2 项私有视频失败）：任务仍是 `done / 100.0%`，`warnings` 里两条——一条原始错误 + 中文提示，一条「共 2 项成功、其余已跳过」的汇总。

## 重试与日志

失败的任务不用回下载页重新粘贴链接：任务卡片右下角的「重试」会调 `POST /api/jobs/{id}/retry`，后端拿原任务的 `url` / `mode` / `quality` / `format_id` / `playlist` 重新排一个新任务（`JobStore::respawn`），当前设置里的 Cookie / 代理 / 输出目录也一并沿用。

- 所以 `Job` 里保存了 `quality`、`formatId`、`playlist` 三个字段 —— 纯粹为了重试时原样复用（这几个信息本来只在创建那一刻用到，存下来才不用让用户重填）。
- 正在跑（queued / running）的任务点重试会被拒：`400 {"error":"任务还在进行中，不需要重试"}`，避免误点把同一份内容下两遍。
- 重试是「新任务」，旧任务记录保留，方便对比两次的报错。

任务卡片上的「日志」按钮对应 `GET /api/jobs/{id}/logs`（这个接口早就有，之前界面上一直没入口）。里面按时间顺序列：

1. **完整命令行** —— 过滤掉进度模板后的真实 yt-dlp 调用，包含当时用的 Cookie 来源、JS 运行时、输出路径；
2. `ERROR:` 原始报错行 —— 卡片上的 `error` 是 `friendly_error()` 翻译过的中文提示，排查还是得看原文；
3. 结束态标记 —— `完成：<文件>` / `任务失败：<原因>` / `任务已取消` / `下载线程崩溃：<原因>`。

日志每任务最多保留 300 行（超出丢最旧的），随任务一起被 `prune()` 的 200 条上限回收。

实测：一个指向不存在视频的任务，卡片上 `error` 是中文提示，日志弹窗里能看到 `ERROR: [youtube] zzzzzzzzzzz: This video is unavailable` 与 `任务失败：…`；点「重试」后 toast 提示「已重新排入队列」并生成新任务 ID。

## 配置

优先级：环境变量 > `~/.config/lycodl/settings.json` > 默认值。Web 设置页写的就是这个文件。

| 环境变量 | 作用 |
| --- | --- |
| `LYCODL_CONFIG_DIR` | 配置目录（默认 `~/.config/lycodl`） |
| `LYCODL_DATA_DIR` | 数据目录，托管内核放这里（默认 `~/.local/share/lycodl`） |
| `LYCODL_OUTPUT_DIR` | 覆盖输出目录 |
| `LYCODL_YTDLP` | 指定内核二进制路径（优先级最高） |
| `LYCODL_JS_RUNTIME` | 显式指定 JS 运行时，形如 `node:/abs/path/to/node` |
| `LYCODL_PORT` | Web 端口（默认 8765，其次读 `LILYCO_PORT`；被占用时自动往后找可用端口） |
| `LYCODL_NO_OPEN` | 设为 `1` 则不自动开浏览器 |
| `LYCODL_KERNEL_FORCE` | 设为 `1` 则 `kernel-update` 无视版本号强制重装内核 |
| `LYCODL_FFMPEG` | 指定 ffmpeg 可执行文件（默认从 PATH 找，找不到再退常见路径） |
| `LYCODL_BROWSER` | 浏览器桥用的 Chromium 可执行文件（默认依次找 chromium / chrome / edge / brave） |

`settings.json` 里的 `impersonate` 字段对应设置页的「浏览器指纹伪装」（值如 `chrome` / `safari`，`null` 表示关闭）。它只在遇到 Cloudflare 类 403 时才需要打开。

`browserBridge`（布尔，默认 `false`）与 `flaresolverrEndpoint`（字符串或 `null`，默认关闭）对应设置页「直链兜底」。留空或填 `none` 都会存成 `null`。这两个开关只影响**解析失败后的降级尝试**，不改变正常下载路径。

## YouTube 的两个硬前提

这不是本项目的 bug，是 YouTube 现状：

1. **必须给 Cookie** —— 设置页「从浏览器取 Cookie」选一个本机已登录 YouTube 的浏览器；或设置里配代理。
2. **必须有 JavaScript 运行时** —— 解签名要用。装 node / deno / bun 任一即可，程序会自动探测（包括 nvm / volta / fnm / bun 的安装路径）。缺了它，报错是误导性的 `No video formats found!`。

两类错误都会被翻译成中文提示，告诉你下一步点哪里。

## 安全边界

lilyco 的安全门在命令注册时就固定了，分级如下：

- **Web / CLI / TUI** 用 `DownloadOnly` 策略：放行 T0 + T1，拒绝 T2/T3。写操作只落配置目录和用户下载目录，不碰系统面。
- **MCP** 用 `Agent` 策略：只放行 T0，外加 `download` / `probe` / `settings-set` 三个白名单。`kernel-update` 刻意**不在**白名单里 —— 换内核属于人类决策。

服务只绑 `127.0.0.1`。

## 代码结构

```
src/
  main.rs      四端分发（Web / CLI / TUI / MCP）
  commands.rs  8 个 #[derive(App)] 命令 + 注册表装配
  api.rs       axum 路由：REST + SSE + 静态资源
  jobs.rs      任务表、并发槽位、进度广播、重试（respawn）
  ytdlp.rs     内核探测 / 探活 / 解析（含降级链）/ 下载 / 校验 / 热更新
  caps.rs      运行环境自检（JS 运行时 / ffmpeg / 指纹伪装 / Cookie 源 / 提取器 / 磁盘）
  sites.rs     36 个主流站点目录 + 按 URL 匹配 + 按站记住生效的解析策略
  browser.rs   CDP 浏览器桥（Cookie / UA / 媒体嗅探）+ FlareSolverr HTML 直链提取
  policy.rs    DownloadOnly 与 Agent 两条安全策略
  settings.rs  配置读写
  paths.rs     目录约定
  assets.rs    include_dir 托管前端产物
web/           React 19 + TS + Tailwind v4 + shadcn/ui
```
