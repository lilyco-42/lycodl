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

五个页签：**下载**（粘贴链接 → 解析 → 选模式/画质 → 提交）、**任务**（SSE 实时进度、取消）、**文件**（成品列表、直接下载、定位到文件夹）、**设置**（输出目录、浏览器 Cookie、代理、并发数、内核更新）、**命令（AI）**（把注册表当工具签名展示，可复制 CLI 用法给 AI）。

「定位」走的是 `POST /api/reveal`：后端用 D-Bus 的 `org.freedesktop.FileManager1.ShowItems` 让 Dolphin / Nautilus 直接**选中**那个文件（实测拉起 `dolphin --select <路径>`），拉不起来就退一步 `xdg-open` 打开所在目录，再不行前端退回「复制路径到剪贴板」。该接口只允许定位输出目录内的路径，不能拿来当任意文件的打开原语。

前端构建产物通过 `include_dir!` 编进二进制，单个可执行文件即可运行（health 里的 `webMode` 会显示 `embedded`）。

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
  jobs.rs      任务表、并发信号量、进度广播
  ytdlp.rs     内核探测 / 探活 / 解析 / 下载 / 校验 / 热更新
  policy.rs    DownloadOnly 与 Agent 两条安全策略
  settings.rs  配置读写
  paths.rs     目录约定
  assets.rs    include_dir 托管前端产物
web/           React 19 + TS + Tailwind v4 + shadcn/ui
```
