# 本机 Chrome CDP 与 lycodl 发布登录态

## 结论

Chrome 136 起，Google Chrome 拒绝在默认 `user-data-dir` 上开启 `--remote-debugging-port` / `--remote-debugging-pipe`。这不是权限或策略问题，`RemoteDebuggingAllowed` 策略也不能放行默认目录；用 symlink、`XDG_CONFIG_HOME`、`CHROME_CONFIG_HOME`、`CHROME_USER_DATA_DIR` 等别名绕路在本机实测均不稳定或被拒绝。

因此 lycodl 不改动、也不重启桌面主 Chrome，而是保留一个专用非默认调试 profile：

- `~/.cache/lycodl-cdp-profile`，由 `lycodl-cdp.service` 在 `127.0.0.1:9333` 启动；
- `lycodl.service` 仍是唯一用户开关，`lycodl-cdp.service` 是 `PartOf=lycodl.service` 的 static 内部单元，不要单独 enable；
- 每次启动 CDP 之前，`scripts/sync-chrome-cookies.py` 把桌面主 Chrome 的 `Default/Cookies` 合并进专用 profile。
- `lycodl-cdp.service` 设为 `Restart=always` + `RestartSec=3`，连 0 退出也会自动拉起；`lycodl.service` 停止时仍通过 `PartOf` 连带停止。

## Cookie 合并策略

脚本用 SQLite `backup()` 对主库和 CDP 库做一致性快照，按 Chrome cookie 唯一键 (`host_key`, `top_frame_site_key`, `has_cross_site_ancestor`, `name`, `path`, `source_scheme`, `source_port`) 合并：

- 主 Chrome 较新的行覆盖 CDP 旧行，导入新登录；
- CDP 独有或较新的行保留，不丢平台发布过程中产生的 cookie；
- 比较 `last_update_utc`，重复执行幂等；
- 不覆盖 `Local State` / `os_crypt`，因为两 profile 共用本机 Linux 钥匙环密钥，复制密文后可正常解密；
- 只对桌面主 Chrome 的行数设下限（`LYCODL_MIN_COOKIES`，默认 100）：主 profile 太薄时跳过合并；
- CDP profile 行数少、甚至为 0 时**不**阻止合并 —— 合并是加法式的（按键并入后做 union 校验），这种状态正是需要自愈的场景；
- 任何读取/校验失败都跳过合并并返回 0，不阻塞 Chrome 启动；
- 不保存、不打印 cookie 值或凭据。

## 部署

```bash
cd /path/to/lycodl
install -Dm0755 scripts/sync-chrome-cookies.py "$HOME/.local/share/lycodl/sync-chrome-cookies.py"
install -Dm0644 scripts/lycodl.service.example "$HOME/.config/systemd/user/lycodl.service"
install -Dm0644 scripts/lycodl-cdp.service.example "$HOME/.config/systemd/user/lycodl-cdp.service"
systemctl --user daemon-reload
systemctl --user enable --now lycodl.service
```

示例单元使用 systemd 的 `%h` 展开到当前用户家目录；用户级 systemd 默认提供 `HOME`，同步脚本据此读取 `%h/.config/google-chrome` 与 `%h/.cache/lycodl-cdp-profile`。不要写 `Environment=HOME=%h`：实测 `Environment` 值不展开 `%h`，会变成字面量 `%h`。非默认布局可用 `LYCODL_MAIN_CHROME_DIR`、`LYCODL_CDP_CHROME_DIR`、`LYCODL_CHROME_PROFILE` 覆盖。`lycodl-cdp.service` 不带 `[Install]` 段，保持 static。校验：

```bash
systemd-analyze --user verify "$HOME/.config/systemd/user/lycodl.service"
systemd-analyze --user verify "$HOME/.config/systemd/user/lycodl-cdp.service"
python3 scripts/test-sync-chrome-cookies.py
curl -fsS http://127.0.0.1:8766/api/health
curl -fsS http://127.0.0.1:9333/json/version
```

## 使用与排障

- 桌面主 Chrome 里新登录平台后，需要重启 `lycodl.service` 才会把新 cookie 合并到 9333；停止/启动 `lycodl.service` 会自动连带停止/启动 CDP 单元。
- 如果 CDP profile 的 `Default/Cookies` 被清空（本机 2026-11-15 出现过只剩 39 行 Google cookie 的情况），重启 `lycodl.service` 会自动从桌面主 Chrome 恢复；旧版本要求两边都不少于 100 行，会导致永久拒跑，已修复并有回归测试覆盖。
- 不要把两个 Chrome 实例指向同一个默认 profile，避免 `SingletonLock` 与 SQLite 并发写坏。
- 发布页「发布前筛选」会只读检查 CDP 登录 cookie、夸克账号网盘列表、平台发布器与本地 ledger；不会真发。真实发布前 driver 会自检 CDP，9333 离线时尝试 `systemctl --user start lycodl-cdp.service` 并等待约 20 秒；仍失败则该平台标记不可用，不影响夸克上传。
- `/api/health` 每 15 秒被 UI 轮询，返回 `cdpReady` / `cdpDetail` / `cdpUrl`；页头显示「发布浏览器在线/离线」，发布页在真发模式下显示离线提示。
- `publishDryRun=false` 时一键发布可能真发；只想看效果请先在设置里打开演练模式。
