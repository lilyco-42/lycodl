/* lycodl page bridge —— 让本地控制台（127.0.0.1:8766）可以调用扩展里的 adapter 执行器。
 *
 * 网页 -> content script: window.postMessage({source:"lycodl", type:"runAdapter", requestId, adapter, payload}, origin)
 * content -> background: chrome.runtime.sendMessage({type:"lycodlRunAdapter", adapter, payload})
 * content -> 网页: window.postMessage({source:"lycodl", type:"runAdapterResult", requestId, ok, result|error}, origin)
 *
 * 只在本机 origin 上回话，避免被任意网页利用。
 */
(function () {
  var PAGE_ORIGIN = /^http:\/\/(127\.0\.0\.1|localhost):\d+$/;

  function reply(origin, data) {
    try {
      window.postMessage(data, origin);
    } catch (ignore) {}
  }

  window.addEventListener("message", function (event) {
    if (!PAGE_ORIGIN.test(event.origin)) return;
    var data = event.data;
    if (!data || data.source !== "lycodl" || data.type !== "runAdapter") return;
    var requestId = data.requestId || String(Date.now());
    if (!data.adapter) {
      reply(event.origin, { source: "lycodl", type: "runAdapterResult", requestId: requestId, ok: false, error: "adapter missing" });
      return;
    }
    chrome.runtime.sendMessage(
      { type: "lycodlRunAdapter", adapter: data.adapter, payload: data.payload || {} },
      function (response) {
        var err = chrome.runtime.lastError;
        if (err) {
          reply(event.origin, { source: "lycodl", type: "runAdapterResult", requestId: requestId, ok: false, error: err.message || String(err) });
          return;
        }
        reply(event.origin, {
          source: "lycodl",
          type: "runAdapterResult",
          requestId: requestId,
          ok: !!(response && response.ok),
          result: response && response.result,
          error: response && response.error,
        });
      },
    );
  });
})();