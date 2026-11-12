/* lycodl 扩展弹窗：只负责界面与存储，所有网络请求都交给 background（Origin 才是 chrome-extension://） */

var els = {
  token: document.getElementById("token"),
  toggle: document.getElementById("toggle-token"),
  port: document.getElementById("port"),
  save: document.getElementById("save"),
  test: document.getElementById("test"),
  send: document.getElementById("send"),
  status: document.getElementById("status"),
  modes: document.querySelectorAll('input[name="mode"]'),
};

function setStatus(text, kind) {
  els.status.textContent = text;
  els.status.className = kind === "ok" ? "ok" : kind === "err" ? "err" : "";
}

function sendMessage(message) {
  return new Promise(function (resolve) {
    chrome.runtime.sendMessage(message, function (response) {
      void chrome.runtime.lastError;
      resolve(response || { ok: false, error: "后台脚本没有响应，请重新加载扩展。" });
    });
  });
}

function readForm() {
  var mode = "video";
  for (var i = 0; i < els.modes.length; i += 1) {
    if (els.modes[i].checked) {
      mode = els.modes[i].value;
    }
  }
  var portText = els.port.value.trim();
  var port = portText === "" ? null : parseInt(portText, 10);
  return {
    token: els.token.value.trim(),
    port: Number.isInteger(port) && port > 0 && port < 65536 ? port : null,
    mode: mode === "audio" ? "audio" : "video",
  };
}

async function saveSettings() {
  var form = readForm();
  var response = await sendMessage({
    type: "saveSettings",
    token: form.token,
    port: form.port,
    mode: form.mode,
  });
  if (response && response.ok) {
    setStatus("设置已保存。" + (form.port === null ? "端口：自动发现" : "端口：" + form.port), "ok");
  } else {
    setStatus((response && response.error) || "保存失败。", "err");
  }
  return response;
}

function setBusy(busy, text) {
  els.save.disabled = busy;
  els.test.disabled = busy;
  els.send.disabled = busy;
  if (text) {
    setStatus(text, "");
  }
}

async function testConnection() {
  await saveSettings();
  setBusy(true, "正在连接 lycodl…");
  var response = await sendMessage({ type: "test", token: readForm().token });
  setBusy(false);
  if (response && response.ok) {
    var health = response.health || {};
    var lines = [
      "连接成功：" + (health.name || "lycodl") + " " + (health.version || ""),
      "端口：" + response.port + (response.discovered ? "（自动发现）" : ""),
    ];
    if (health.ytdlpVersion) {
      lines.push("yt-dlp：" + health.ytdlpVersion);
    }
    setStatus(lines.join("\n"), "ok");
  } else {
    setStatus((response && response.error) || "连接失败。", "err");
  }
}

async function sendCurrentPage() {
  await saveSettings();
  setBusy(true, "正在解析当前页面…（可能需要十几秒，结果会以系统通知提示）");
  var response = await sendMessage({ type: "send", mode: readForm().mode });
  setBusy(false);
  if (response && response.ok) {
    setStatus("已加入下载队列" + (response.jobId ? "：" + response.jobId : "。"), "ok");
  } else {
    setStatus((response && response.error) || "发送失败。", "err");
  }
}

function toggleToken() {
  var hidden = els.token.type === "password";
  els.token.type = hidden ? "text" : "password";
  els.toggle.textContent = hidden ? "隐藏" : "显示";
}

async function init() {
  var data = await new Promise(function (resolve) {
    chrome.storage.local.get(["token", "port", "mode"], function (value) {
      resolve(value || {});
    });
  });
  if (typeof data.token === "string") {
    els.token.value = data.token;
  }
  if (typeof data.port === "number" && data.port > 0) {
    els.port.value = String(data.port);
  }
  var mode = data.mode === "audio" ? "audio" : "video";
  for (var i = 0; i < els.modes.length; i += 1) {
    els.modes[i].checked = els.modes[i].value === mode;
  }
  if (els.token.value === "") {
    setStatus("还没有令牌：先打开 lycodl 网页 →「设置」→「浏览器扩展」→ 复制插件令牌，粘贴到上面。");
  }
}

els.toggle.addEventListener("click", toggleToken);
els.save.addEventListener("click", function () {
  void saveSettings();
});
els.test.addEventListener("click", function () {
  void testConnection();
});
els.send.addEventListener("click", function () {
  void sendCurrentPage();
});
els.token.addEventListener("keydown", function (event) {
  if (event.key === "Enter") {
    void testConnection();
  }
});

void init();
