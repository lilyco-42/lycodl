/* lycodl 浏览器扩展 —— background service worker
 * 必须是 classic worker（manifest 里没有 "type": "module"，所以这里不要写 import）。
 *
 * 为什么所有 fetch 都放在这里：扩展的 Origin 是 chrome-extension://<id>，
 * 后端只对 chrome-extension:// / moz-extension:// 放行；content script 的 Origin
 * 是网页域名，请求会被后端 403，所以这一版干脆没有 content script。
 */

var HOST = "127.0.0.1";
var PORT_START = 8765;
var PORT_END = 8785;
var HEALTH_TIMEOUT_MS = 2500;
var PROBE_TIMEOUT_MS = 120000;
var DOWNLOAD_TIMEOUT_MS = 30000;
var MENU_ID = "lycodl-send";

/* 通知图标用扩展包内的 PNG（chrome.notifications 对 data: URL 支持不稳，用 getURL 最保险） */
function notifyIconUrl() {
  try {
    return chrome.runtime.getURL("icons/icon-128.png");
  } catch (ignore) {
    return undefined;
  }
}

/* ---------------- 存储 ---------------- */

function storageGet(keys) {
  return new Promise(function (resolve) {
    chrome.storage.local.get(keys, function (data) {
      resolve(data || {});
    });
  });
}

function storageSet(patch) {
  return new Promise(function (resolve) {
    chrome.storage.local.set(patch, function () {
      resolve();
    });
  });
}

function normalizePort(value) {
  var port = typeof value === "number" ? value : parseInt(String(value || ""), 10);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    return null;
  }
  return port;
}

function normalizeMode(value) {
  return value === "audio" ? "audio" : "video";
}

/* 读取配置：token / port（null = 自动发现）/ mode */
async function getConfig() {
  var data = await storageGet(["token", "port", "mode"]);
  return {
    token: typeof data.token === "string" ? data.token.trim() : "",
    port: normalizePort(data.port),
    mode: normalizeMode(data.mode),
  };
}

/* ---------------- HTTP ---------------- */

/* 统一请求封装：总是带 X-Lycodl-Token（后端要求），有 body 时带 JSON Content-Type */
async function request(options) {
  var port = options.port;
  var token = options.token;
  var path = options.path;
  var method = options.method || "GET";
  var body = options.body;
  var timeout = options.timeout || 15000;

  var controller = new AbortController();
  var timer = setTimeout(function () {
    controller.abort();
  }, timeout);

  var headers = { "X-Lycodl-Token": token };
  if (body !== undefined && body !== null) {
    headers["Content-Type"] = "application/json";
  }

  try {
    var response = await fetch("http://" + HOST + ":" + port + path, {
      method: method,
      headers: headers,
      body: body === undefined || body === null ? undefined : JSON.stringify(body),
      signal: controller.signal,
      cache: "no-store",
    });

    var payload = null;
    try {
      payload = await response.json();
    } catch (ignore) {
      payload = null;
    }

    return { status: response.status, ok: response.ok, data: payload, networkError: false };
  } catch (cause) {
    return { status: 0, ok: false, data: null, networkError: true };
  } finally {
    clearTimeout(timer);
  }
}

function errorText(result) {
  if (result && result.networkError) {
    return "无法连接 lycodl（" + HOST + ":" + result.port + "），请确认程序正在运行。";
  }
  var data = result && result.data;
  if (data && typeof data.error === "string" && data.error !== "") {
    return data.error;
  }
  if (data && data.ok === false && typeof data.message === "string") {
    return data.message;
  }
  if (result && result.status === 401) {
    return "令牌无效或已过期，请在 lycodl 网页设置页重新复制插件令牌并粘贴到插件里。";
  }
  return "请求失败（HTTP " + (result ? result.status : 0) + "）。";
}

/* ---------------- 端口发现 ---------------- */

async function probeHealth(port, token) {
  var result = await request({
    port: port,
    token: token,
    path: "/api/health",
    timeout: HEALTH_TIMEOUT_MS,
  });
  if (result.networkError) {
    return null;
  }
  if (result.status === 401) {
    /* 端口上确实有 lycodl，但令牌不对 */
    return { unauthorized: true, port: port };
  }
  if (!result.ok || !result.data || result.data.name !== "lycodl") {
    return null;
  }
  return { unauthorized: false, port: port, health: result.data };
}

/* 端口：先用记住的，不行就从 8765 扫到 8785（后端端口被占用会往后顺延） */
async function resolvePort(token) {
  var config = await getConfig();

  if (!token) {
    throw new Error("还没有填写插件令牌：打开 lycodl 网页的「设置 → 浏览器扩展」复制令牌，再粘贴到插件弹窗里。");
  }

  if (config.port !== null) {
    var cached = await probeHealth(config.port, token);
    if (cached !== null) {
      if (cached.unauthorized) {
        throw new Error("令牌无效或已过期，请在 lycodl 网页设置页重新复制插件令牌。");
      }
      return { port: cached.port, health: cached.health, discovered: false };
    }
  }

  var unauthorizedPort = null;

  for (var port = PORT_START; port <= PORT_END; port += 1) {
    var found = await probeHealth(port, token);
    if (found === null) {
      continue;
    }
    if (found.unauthorized) {
      /* 这个端口上有 lycodl 但令牌不对；先记下来继续扫，
         万一别的端口上另有一个接受该令牌的实例，就优先用那个。 */
      unauthorizedPort = port;
      continue;
    }
    await storageSet({ port: port });
    return { port: port, health: found.health, discovered: true };
  }

  if (unauthorizedPort !== null) {
    throw new Error(
      "在端口 " + unauthorizedPort + " 上找到了 lycodl，但令牌无效或已过期，请在网页设置页重新复制插件令牌。"
    );
  }

  throw new Error(
    "没有在 " + PORT_START + "-" + PORT_END + " 端口上找到 lycodl，请确认程序已启动。"
  );
}

/* ---------------- 通知 / 徽章 ---------------- */

function notify(title, message) {
  try {
    chrome.notifications.create(
      "",
      {
        type: "basic",
        iconUrl: notifyIconUrl(),
        title: title,
        message: message,
        priority: 1,
      },
      function () {
        /* 读一下 lastError，避免控制台噪音 */
        void chrome.runtime.lastError;
      }
    );
  } catch (ignore) {
    /* 通知失败不影响主流程 */
  }
}

function setBadge(text) {
  try {
    chrome.action.setBadgeBackgroundColor({ color: "#8b5cf6" });
    chrome.action.setBadgeText({ text: text });
  } catch (ignore) {}
}

function clearBadgeLater(delay) {
  setTimeout(function () {
    setBadge("");
  }, delay || 4000);
}

/* ---------------- 发送（两段式：probe → download） ---------------- */

async function sendUrl(rawUrl, requestedMode) {
  var url = typeof rawUrl === "string" ? rawUrl.trim() : "";
  if (!/^https?:\/\//i.test(url)) {
    notify("链接不支持", "只支持 http / https 链接：" + (url || "(空)"));
    return { ok: false, error: "只支持 http / https 链接" };
  }

  var config = await getConfig();
  var mode = normalizeMode(requestedMode || config.mode);

  setBadge("…");
  notify("正在解析…", mode === "audio" ? "正在解析音频信息，可能需要十几秒。" : "正在解析视频信息，可能需要十几秒。");

  var port;
  try {
    var target = await resolvePort(config.token);
    port = target.port;
  } catch (cause) {
    setBadge("!");
    clearBadgeLater(6000);
    var message = cause && cause.message ? cause.message : String(cause);
    notify("lycodl 连接失败", message);
    return { ok: false, error: message };
  }

  var probe = await request({
    port: port,
    token: config.token,
    path: "/api/probe",
    method: "POST",
    body: { url: url },
    timeout: PROBE_TIMEOUT_MS,
  });

  if (!probe.ok || !probe.data || probe.data.ok !== true || !probe.data.info) {
    setBadge("!");
    clearBadgeLater(6000);
    var probeError = errorText({
      status: probe.status,
      data: probe.data,
      networkError: probe.networkError,
      port: port,
    });
    notify("解析失败", probeError);
    return { ok: false, error: probeError };
  }

  var info = probe.data.info;
  /* 直链兜底：extractor 全挂时后端会给 direct，必须原样回传，否则下载会退回早已失败的 extractor */
  var direct = probe.data.direct || null;
  var directTitle = direct !== null ? String(info.title || url) : null;
  var title = String(info.title || url);

  setBadge("…");
  notify("解析完成，开始下载…", title);

  var job = await request({
    port: port,
    token: config.token,
    path: "/api/download",
    method: "POST",
    body: {
      url: url,
      mode: mode,
      quality: "best",
      formatId: null,
      playlist: false,
      direct: direct,
      directTitle: directTitle,
    },
    timeout: DOWNLOAD_TIMEOUT_MS,
  });

  if (!job.ok || !job.data || job.data.ok !== true) {
    setBadge("!");
    clearBadgeLater(6000);
    var jobError = errorText({
      status: job.status,
      data: job.data,
      networkError: job.networkError,
      port: port,
    });
    notify("加入下载队列失败", jobError);
    return { ok: false, error: jobError };
  }

  var jobId = typeof job.data.jobId === "string" ? job.data.jobId : "";
  notify("已加入下载队列", title + (jobId ? "\n任务 " + jobId : ""));
  setBadge("OK");
  clearBadgeLater(3000);

  return { ok: true, jobId: jobId, title: title, port: port, mode: mode };
}

/* ---------------- 测试连接 ---------------- */

async function testConnection(overrideToken) {
  var config = await getConfig();
  var token = (overrideToken || config.token || "").trim();
  try {
    var target = await resolvePort(token);
    return {
      ok: true,
      port: target.port,
      discovered: target.discovered,
      health: target.health,
      message:
        "连接成功：" +
        (target.health && target.health.name ? target.health.name : "lycodl") +
        " " +
        (target.health && target.health.version ? target.health.version : "") +
        " @ 端口 " +
        target.port,
    };
  } catch (cause) {
    return { ok: false, error: cause && cause.message ? cause.message : String(cause) };
  }
}

/* 当前活动页（popup / 快捷键 / action 都走这里） */
async function activeTabUrl() {
  return new Promise(function (resolve) {
    chrome.tabs.query({ active: true, currentWindow: true }, function (tabs) {
      var tab = tabs && tabs[0];
      resolve(tab && tab.url ? tab.url : "");
    });
  });
}

async function sendActiveTab(requestedMode) {
  var url = await activeTabUrl();
  if (!url) {
    notify("没有可发送的页面", "当前标签页没有可用的地址。");
    return { ok: false, error: "当前标签页没有可用地址" };
  }
  return sendUrl(url, requestedMode);
}

/* ---------------- 生命周期 ---------------- */

chrome.runtime.onInstalled.addListener(function () {
  chrome.contextMenus.removeAll(function () {
    chrome.contextMenus.create({
      id: MENU_ID,
      title: "发送到 lycodl",
      contexts: ["link", "video", "audio", "page"],
    });
  });
});

chrome.contextMenus.onClicked.addListener(function (info) {
  if (info.menuItemId !== MENU_ID) {
    return;
  }
  var url = info.linkUrl || info.srcUrl || info.pageUrl || "";
  void sendUrl(url, "");
});

if (chrome.commands && chrome.commands.onCommand) {
  chrome.commands.onCommand.addListener(function (command) {
    if (command === "send-current-page") {
      void sendActiveTab("");
    }
  });
}

/* action 有 popup，点击不会触发 onClicked；用户直接用 popup 里的按钮或右键菜单 */
chrome.runtime.onMessage.addListener(function (message, sender, sendResponse) {
  var type = message && message.type;

  if (type === "send") {
    var task = message.url ? sendUrl(message.url, message.mode) : sendActiveTab(message.mode);
    task.then(sendResponse, function (cause) {
      sendResponse({ ok: false, error: cause && cause.message ? cause.message : String(cause) });
    });
    return true;
  }

  if (type === "sendActiveTab") {
    sendActiveTab(message && message.mode).then(sendResponse, function (cause) {
      sendResponse({ ok: false, error: cause && cause.message ? cause.message : String(cause) });
    });
    return true;
  }

  if (type === "test") {
    testConnection(message && message.token).then(sendResponse, function (cause) {
      sendResponse({ ok: false, error: cause && cause.message ? cause.message : String(cause) });
    });
    return true;
  }

  if (type === "saveSettings") {
    var patch = {};
    if (typeof message.token === "string") {
      patch.token = message.token.trim();
    }
    if (message.port !== undefined) {
      patch.port = normalizePort(message.port);
    }
    if (message.mode !== undefined) {
      patch.mode = normalizeMode(message.mode);
    }
    storageSet(patch).then(function () {
      sendResponse({ ok: true });
    });
    return true;
  }

  if (type === "getStatus") {
    getConfig().then(function (config) {
      sendResponse({
        ok: true,
        hasToken: config.token !== "",
        port: config.port,
        mode: config.mode,
      });
    });
    return true;
  }

  return false;
});