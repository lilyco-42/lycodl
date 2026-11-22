/* lycodl adapter executor —— 在浏览器扩展内用 chrome.debugger 执行声明式 adapter。
 *
 * 与 publish-driver/adapter-runner.mjs 共用同一份 adapter JSON；
 * 区别只在执行通道：这里走 chrome.debugger（等同 CDP），不依赖 Node。
 * 由 background.js 通过 importScripts 引入，暴露 self.lycodlRunAdapter。
 */
(function () {
  var DEBUGGER_VERSION = "1.3";

  function sleep(ms) {
    return new Promise(function (resolve) {
      setTimeout(resolve, ms);
    });
  }

  function sliceBytes(s, n) {
    if (!n) return String(s || "");
    var buf = new TextEncoder().encode(String(s || ""));
    if (buf.length <= n) return String(s || "");
    return new TextDecoder("utf-8", { fatal: false }).decode(buf.subarray(0, n));
  }

  function expand(value, vars) {
    return String(value == null ? "" : value).replace(
      /\$([a-zA-Z][a-zA-Z0-9]*)/g,
      function (m, k) {
        return Object.prototype.hasOwnProperty.call(vars, k) ? String(vars[k]) : m;
      },
    );
  }

  function textsExpr(texts) {
    return "[" + (texts || []).map(function (t) { return JSON.stringify(t); }).join(",") + "]";
  }

  function send(tabId, method, params) {
    return new Promise(function (resolve, reject) {
      chrome.debugger.sendCommand({ tabId: tabId }, method, params || {}, function (result) {
        var err = chrome.runtime.lastError;
        if (err) reject(new Error(err.message || String(err)));
        else resolve(result);
      });
    });
  }

  async function evaluate(tabId, expr) {
    var r = await send(tabId, "Runtime.evaluate", {
      expression: expr,
      returnByValue: true,
      awaitPromise: true,
    });
    return r && r.result ? r.result.value : undefined;
  }

  async function waitFor(tabId, test) {
    var timeoutMs = Number(test.timeoutMs || 180000);
    var intervalMs = Number(test.intervalMs || 1500);
    var expr = test.expr
      || "(function(){var t=document.body.innerText||\"\";var want=" + textsExpr(test.texts)
        + ";for(var i=0;i<want.length;i++){if(t.indexOf(want[i])>=0)return \"ok\";}"
        + (test.urlIncludes ? "if(location.href.indexOf(" + JSON.stringify(test.urlIncludes) + ")>=0)return \"ok\";" : "")
        + "return false;})()";
    var deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      var ok = await evaluate(tabId, expr).catch(function () { return false; });
      if (ok) return true;
      await sleep(intervalMs);
    }
    return false;
  }

  async function setFile(tabId, selector, filePath) {
    var doc = await send(tabId, "DOM.getDocument", { depth: -1 });
    var q = await send(tabId, "DOM.querySelector", { nodeId: doc.root.nodeId, selector: selector });
    if (!q || !q.nodeId) throw new Error("file input not found: " + selector);
    await send(tabId, "DOM.setFileInputFiles", { files: [filePath], nodeId: q.nodeId });
  }

  async function fill(tabId, selector, value) {
    var focused = await evaluate(
      tabId,
      "(function(){var e=document.querySelector(" + JSON.stringify(selector)
        + ");if(!e)return false;e.focus();try{if(e.select)e.select();}catch(x){}return true;})()",
    );
    if (!focused) return false;
    if (value) await send(tabId, "Input.insertText", { text: String(value) });
    await sleep(300);
    return true;
  }

  async function clickByText(tabId, texts, opts) {
    var selector = (opts && opts.selector) || "button,a,div,span";
    var expr = "(function(){var want=" + textsExpr(texts) + ";var list=document.querySelectorAll("
      + JSON.stringify(selector) + ");var cand=[];for(var i=0;i<list.length;i++){var e=list[i];"
      + "var t=(e.innerText||e.textContent||\"\").trim();if(want.indexOf(t)<0)continue;"
      + "if(e.disabled)continue;if(!e.getClientRects().length)continue;cand.push(e);}"
      + "if(!cand.length)return null;var b=cand[cand.length-1];try{b.scrollIntoView({block:'center'});}catch(x){}"
      + "b.click();return (b.innerText||'').trim().slice(0,20);})()";
    return evaluate(tabId, expr);
  }

  async function clickDeep(tabId, texts, preferTag) {
    var expr = "(function(){var want=" + textsExpr(texts) + ";function all(root,out,d){if(d>8)return;"
      + "var list;try{list=root.querySelectorAll('*');}catch(e){return;}for(var i=0;i<list.length;i++){"
      + "var e=list[i];out.push(e);if(e.shadowRoot)all(e.shadowRoot,out,d+1);else if(e._sr)all(e._sr,out,d+1);}}"
      + "var nodes=[];all(document,nodes,0);var cand=[];for(var i=0;i<nodes.length;i++){var e=nodes[i];"
      + "var t=(e.innerText||e.textContent||'').trim();if(want.indexOf(t)<0)continue;if(e.disabled)continue;"
      + "if(!e.getClientRects().length)continue;cand.push(e);}if(!cand.length)return null;"
      + "var pick=" + JSON.stringify(preferTag || "") + "?cand.filter(function(e){return e.tagName==="
      + JSON.stringify(preferTag || "") + ";}):[];var b=pick.length?pick[0]:cand[cand.length-1];"
      + "try{b.scrollIntoView({block:'center'});}catch(x){}b.click();"
      + "return {t:(b.innerText||'').trim(),tag:b.tagName,n:cand.length};})()";
    return evaluate(tabId, expr);
  }

  async function runStep(tabId, step, vars) {
    switch (step.action) {
      case "goto":
        await send(tabId, "Page.navigate", { url: expand(step.url, vars) });
        return true;
      case "waitFor":
        return waitFor(tabId, { expr: step.expr, timeoutMs: step.timeoutMs, intervalMs: step.intervalMs });
      case "waitMs":
        await sleep(Number(step.ms || 0));
        return true;
      case "setFile":
        await setFile(tabId, step.selector || "input[type=file]", vars.file);
        return true;
      case "fill":
        return fill(tabId, step.selector, sliceBytes(expand(step.value, vars), step.maxBytes));
      case "clickText":
        return clickByText(tabId, (step.texts || []).map(function (t) { return expand(t, vars); }), { selector: step.selector });
      case "clickDeep":
        return clickDeep(tabId, (step.texts || []).map(function (t) { return expand(t, vars); }), step.preferTag);
      case "eval":
        return evaluate(tabId, expand(step.expr, vars));
      case "press":
        await send(tabId, "Input.dispatchKeyEvent", { type: "keyDown", key: step.key || "Enter" });
        await send(tabId, "Input.dispatchKeyEvent", { type: "keyUp", key: step.key || "Enter" });
        return true;
      default:
        throw new Error("unknown adapter action: " + step.action);
    }
  }

  async function runAdapter(adapter, req) {
    var vars = {
      file: req.file,
      fileName: req.fileName || "",
      title: req.title || "",
      desc: [req.title || "", req.shareUrl ? "网盘：" + req.shareUrl : "", req.sharePasscode ? "提取码：" + req.sharePasscode : ""]
        .filter(Boolean)
        .join(" "),
      shareUrl: req.shareUrl || "",
      sharePasscode: req.sharePasscode || "",
      visibility: req.visibility || "public",
    };

    var tab = await new Promise(function (resolve) {
      chrome.tabs.create({ url: adapter.origin, active: false }, resolve);
    });
    var tabId = tab.id;
    var attached = false;
    try {
      await new Promise(function (resolve, reject) {
        chrome.debugger.attach({ tabId: tabId }, DEBUGGER_VERSION, function () {
          var err = chrome.runtime.lastError;
          if (err) reject(new Error("无法附加调试器（需要用户同意一次）: " + err.message));
          else resolve();
        });
      });
      attached = true;
      await send(tabId, "Page.enable");
      await send(tabId, "Runtime.enable");

      for (var i = 0; i < (adapter.steps || []).length; i += 1) {
        var step = adapter.steps[i];
        try {
          var r = await runStep(tabId, step, vars);
          if (step.action === "waitFor" && !r && !step.optional) {
            return { status: "failed", detail: "adapter 等待超时: " + step.expr };
          }
        } catch (cause) {
          if (!step.optional) throw cause;
        }
      }

      var ok = await waitFor(tabId, adapter.result.success || {});
      var url = adapter.result.urlExpr ? await evaluate(tabId, adapter.result.urlExpr).catch(function () { return null; }) : null;
      if (!ok) {
        var tail = String(await evaluate(tabId, "(document.body.innerText||'').slice(-220)").catch(function () { return ""; }))
          .replace(/\n/g, " | ");
        return { status: "failed", detail: "未捕获发布成功 | " + tail, url: url };
      }
      return { status: "ok", detail: adapter.result.detail || adapter.name + " 已发布", url: url };
    } finally {
      if (attached) {
        try { chrome.debugger.detach({ tabId: tabId }); } catch (ignore) {}
      }
      try { chrome.tabs.remove(tabId); } catch (ignore) {}
    }
  }

  self.lycodlRunAdapter = runAdapter;
})();