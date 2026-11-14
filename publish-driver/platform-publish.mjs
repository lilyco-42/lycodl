'use strict';
// lycodl 平台发布提交器：通过 CDP 驱动已登录的浏览器完成各平台投稿。
// 输入 payload: {dest, file, fileName, title, shareUrl, sharePasscode, cdpUrl, draft, timeoutSecs}
// 输出: {status:'ok'|'failed'|'unavailable', detail, url}
import { httpJson, Conn, newPage, closePage, truncateBytes } from './cdp-util.mjs';

const NL = String.fromCharCode(10);

function T(expr) { return 'String(' + expr + '||"").split(String.fromCharCode(10)).join(" | ")'; }

async function connectBrowser(cdpUrl) {
  const ver = await httpJson(String(cdpUrl || 'http://127.0.0.1:9333').replace(/\/+$/, '') + '/json/version', 4000);
  if (!ver || !ver.webSocketDebuggerUrl) throw new Error('CDP 未就绪');
  return Conn.connect(ver.webSocketDebuggerUrl);
}

async function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }

async function bodyText(page, n) {
  return page.evaluate('(' + T('document.body.innerText') + ').slice(0,' + (n || 600) + ')');
}

/** 点击第一个「可见且文本精确等于 txt 的可点元素」，支持自定义选择器范围 */
async function clickByText(page, texts, opts) {
  const o = opts || {};
  const expr = '(function(){'
    + 'var texts=' + JSON.stringify(texts) + ';'
    + 'var sel=' + JSON.stringify(o.selector || 'button,span,div,a,li') + ';'
    + 'var nodes=Array.from(document.querySelectorAll(sel));'
    + 'for(var i=0;i<nodes.length;i++){var e=nodes[i];var t=(e.innerText||"").trim();'
    + ' if(texts.indexOf(t)<0) continue;'
    + ' if(' + (o.mustVisible === false ? 'false' : '!(e.getClientRects().length)') + ') continue;'
    + ' if(e.disabled) continue;'
    + ' e.scrollIntoView({block:"center"});e.click();return t;}'
    + 'return null;})()';
  return page.evaluate(expr);
}

// ---------------- bilibili ----------------
async function bilibili(req, browser) {
  const { page, targetId } = await newPage(browser, 'https://member.bilibili.com/platform/upload/video/frame');
  try {
    await page.waitFor('!!document.querySelector("input[type=file]")', 30000);
    await page.setFile('input[type=file]', req.file);
    // 等到出现「上传完成」+ 立即投稿
    await page.waitFor('(function(){var t=document.body.innerText||"";return t.indexOf("上传完成")>=0&&(t.indexOf("立即投稿")>=0||t.indexOf("存草稿")>=0);})()', 900000, 2000);
    await sleep(2500);
    // 关闭可能遮挡的引导弹窗
    await clickByText(page, ['知道了', '我知道了', '暂不考虑', '不用了']);
    await sleep(500);
    // 标题
    const title = truncateBytes(req.title || req.fileName || '视频', 80);
    const titleOk = await page.evaluate('(function(){var e=document.querySelector("input[placeholder*=稿件标题]");if(!e)return false;e.focus();e.select&&e.select();return true;})()');
    if (titleOk) {
      await page.send('Input.insertText', { text: title });
      await page.evaluate('(function(){var e=document.querySelector("input[placeholder*=稿件标题]");if(e){e.dispatchEvent(new Event("input",{bubbles:true}));e.dispatchEvent(new Event("change",{bubbles:true}));}return true;})()');
    }
    // 简介（第一个 ql-editor）
    const desc = truncateBytes((req.title || '') + NL + '网盘：' + (req.shareUrl || '') + '  提取码：' + (req.sharePasscode || ''), 300);
    await page.evaluate('(function(){var e=document.querySelector(".ql-editor");if(!e)return false;e.focus();return true;})()');
    await page.send('Input.insertText', { text: desc });
    await sleep(800);
    const useDraft = !!req.draft;
    const clicked = useDraft
      ? await clickByText(page, ['存草稿'], { selector: 'span,div,button' })
      : await clickByText(page, ['立即投稿'], { selector: 'span,div,button' });
    if (!clicked) return { status: 'failed', detail: '未找到投稿按钮（存草稿/立即投稿）' };
    // 等待结果：草稿成功提示 或 稿件投递成功
    const done = await page.waitFor('(function(){var t=document.body.innerText||"";'
      + 'if(t.indexOf("投稿成功")>=0||t.indexOf("稿件投递成功")>=0)return "published";'
      + 'if(t.indexOf("已保存")>=0||t.indexOf("保存成功")>=0||t.indexOf("草稿箱")>=0)return "draft";'
      + 'return false;})()', 90000, 1000).catch(function (e) { return null; });
    const body = await bodyText(page, 400);
    const bvid = await page.evaluate('(function(){var m=(document.body.innerText||"").match(/BV[0-9A-Za-z]{10}/);return m?m[0]:null;})()');
    const url = bvid ? 'https://www.bilibili.com/video/' + bvid : null;
    if (!done) return { status: 'ok', detail: '已点击' + clicked + '（未捕获成功提示，请人工复核）', url };
    return { status: 'ok', detail: 'B站' + (done === 'draft' ? '已存草稿' : '已投稿') + (bvid ? ' ' + bvid : '') + ' | ' + body.slice(0, 160), url };
  } finally { await closePage(browser, targetId); }
}

async function fillText(page, selector, text, opts) {
  const o = opts || {};
  const ok = await page.evaluate('(function(){var e=document.querySelector(' + JSON.stringify(selector) + ');if(!e)return false;e.focus();'
    + 'if(e.isContentEditable){e.textContent="";}else{e.value="";}return true;})()');
  if (!ok) return false;
  if (text) await page.send('Input.insertText', { text });
  await page.evaluate('(function(){var e=document.querySelector(' + JSON.stringify(selector) + ');if(e){e.dispatchEvent(new Event("input",{bubbles:true}));e.dispatchEvent(new Event("change",{bubbles:true}));}return true;})()');
  if (o.blur !== false) await page.evaluate('(function(){var e=document.querySelector(' + JSON.stringify(selector) + ');if(e&&e.blur)e.blur();return true;})()');
  return true;
}

/** 点击含指定文本的 radio 的 input（单选项必须点 input 而不是 label） */
async function pickRadio(page, text) {
  return page.evaluate('(function(){'
    + 'var want=' + JSON.stringify(text) + ';'
    + 'var labels=Array.from(document.querySelectorAll("label,li,div"));'
    + 'for(var i=0;i<labels.length;i++){var e=labels[i];var t=(e.innerText||"").trim();'
    + ' if(t!==want) continue;'
    + ' var inp=e.querySelector("input[type=radio]")||(e.parentElement?e.parentElement.querySelector("input[type=radio]"):null);'
    + ' if(inp){inp.click();return "radio";}'
    + ' e.click();return "label";}'
    + 'return null;})()');
}

// ---------------- douyin ----------------
async function douyin(req, browser) {
  const { page, targetId } = await newPage(browser, 'https://creator.douyin.com/creator-micro/content/upload');
  try {
    await page.waitFor('!!document.querySelector("input[type=file]")', 60000, 1000);
    await page.setFile('input[type=file]', req.file);
    await page.waitFor('location.href.indexOf("/content/post/video")>=0', 180000, 1500);
    // 等上传完成：出现「重新上传」或「发布」按钮可用
    await page.waitFor('(function(){var t=document.body.innerText||"";var b=Array.from(document.querySelectorAll("button")).filter(function(e){return (e.innerText||"").trim()==="发布";})[0];'
      + 'return (t.indexOf("重新上传")>=0||t.indexOf("上传成功")>=0)&&b&&!b.disabled;})()', 900000, 2500);
    await sleep(2000);
    await clickByText(page, ['知道了', '我知道了', '取消']);
    await sleep(400);
    const title = truncateBytes(req.title || req.fileName || '视频', 30);
    await fillText(page, 'input[placeholder*=作品标题]', title);
    const descFull = String(req.title || '') + NL + '网盘：' + String(req.shareUrl || '') + ' 提取码：' + String(req.sharePasscode || '');
    const desc = truncateBytes(descFull.replace(/[\r\n]+/g, ' '), 300);
    await fillText(page, '.zone-container.editor-kit-container, div[data-placeholder*=作品简介]', desc);
    if (req.visibility === 'private') await pickRadio(page, '仅自己可见');
    await sleep(600);
    const clicked = await page.evaluate('(function(){var b=Array.from(document.querySelectorAll("button")).filter(function(e){return (e.innerText||"").trim()==="发布"&&!e.disabled;})[0];'
      + 'if(!b)return null;b.scrollIntoView({block:"center"});b.click();return "发布";})()');
    if (!clicked) return { status: 'failed', detail: '未找到可用的发布按钮' };
    const done = await page.waitFor('(function(){var t=document.body.innerText||"";if(t.indexOf("发布成功")>=0)return "ok";if(location.href.indexOf("content/manage")>=0)return "ok";'
      + 'if(t.indexOf("作品已发布")>=0)return "ok";return false;})()', 180000, 1500).catch(function () { return null; });
    const body = await bodyText(page, 300);
    if (!done) return { status: 'failed', detail: '点击发布后未捕获成功提示 | ' + body.slice(0, 220) };
    const workId = await page.evaluate('(function(){var m=(document.body.innerText||"").match(/\/video\/([0-9]{15,25})/);return m?m[1]:null;})()').catch(function () { return null; });
    return { status: 'ok', detail: '抖音已发布' + (workId ? ' ' + workId : '') + ' | ' + body.slice(0, 180), url: workId ? 'https://www.douyin.com/video/' + workId : null };
  } finally { await closePage(browser, targetId); }
}

// ---------------- xiaohongshu ----------------
async function xhs(req, browser) {
  const { page, targetId } = await newPage(browser, 'https://creator.xiaohongshu.com/publish/publish?source=official');
  try {
    await page.waitFor('!!document.querySelector("input[type=file]")', 60000, 1000);
    await page.setFile('input[type=file]', req.file);
    // 等上传完成（出现标题输入框）
    await page.waitFor('!!document.querySelector("input.d-text, input[placeholder*=标题]")', 900000, 2500);
    await sleep(3000);
    await clickByText(page, ['我知道了', '知道了']);
    await sleep(500);
    const title = truncateBytes(req.title || req.fileName || '视频', 20);
    await fillText(page, 'input.d-text, input[placeholder*=标题]', title);
    const descFull = String(req.title || '') + NL + '网盘：' + String(req.shareUrl || '') + ' 提取码：' + String(req.sharePasscode || '');
    await fillText(page, '.tiptap.ProseMirror, div[contenteditable=true]', truncateBytes(descFull.replace(/[\r\n]+/g, ' '), 900));
    await sleep(1200);
    // 提交按钮可能位于 closed shadow root
    // 优先点 shadow root 内的 BUTTON.ce-btn，其次最内层的可点节点
    const clicked = await page.evaluate('(function(){'
      + 'function all(root,out,d){ if(d>8)return; var list; try{list=root.querySelectorAll("*");}catch(e){return;}'
      + ' for(var i=0;i<list.length;i++){var e=list[i];out.push(e); if(e.shadowRoot) all(e.shadowRoot,out,d+1); if(e._sr) all(e._sr,out,d+1);} }'
      + 'var nodes=[];all(document,nodes,0);'
      + 'var cand=[];'
      + 'for(var i=0;i<nodes.length;i++){var e=nodes[i];var t=(e.innerText||e.textContent||"").trim();'
      + ' if(t!=="发布"&&t!=="发布笔记") continue;'
      + ' if(e.disabled) continue;'
      + ' cand.push(e);}'
      + 'if(!cand.length)return null;'
      + 'var pick=cand.filter(function(e){return e.tagName==="BUTTON";});'
      + 'if(!pick.length){pick=[cand[cand.length-1]];}'
      + 'var b=pick[0];'
      + 'try{b.scrollIntoView({block:"center"});}catch(x){}'
      + 'b.click();'
      + 'return {t:(b.innerText||"").trim(),tag:b.tagName,n:cand.length};})()');
    if (!clicked) return { status: 'failed', detail: '未找到发布笔记按钮' };
    const clickedLabel = typeof clicked === 'string' ? clicked : (clicked.t + '/' + clicked.tag + '(' + clicked.n + ')');
    const done = await page.waitFor('(function(){var t=document.body.innerText||"";'
      + 'if(t.indexOf("发布成功")>=0||t.indexOf("笔记发布成功")>=0)return "ok";'
      + 'if(location.href.indexOf("publish/success")>=0)return "ok";'
      + 'if(location.href.indexOf("published")>=0)return "ok";'
      + 'return false;})()', 180000, 1500).catch(function () { return null; });
    const body = await bodyText(page, 300);
    if (!done) return { status: 'failed', detail: '点击' + clickedLabel + '后未捕获成功提示 | ' + body.slice(0, 220) };
    return { status: 'ok', detail: '小红书已发布 | ' + body.slice(0, 180) };
  } finally { await closePage(browser, targetId); }
}

// ---------------- kuaishou ----------------
async function kuaishou(req, browser) {
  const { page, targetId } = await newPage(browser, 'https://cp.kuaishou.com/article/publish/video');
  try {
    await page.waitFor('!!document.querySelector("input[type=file]")', 60000, 1000);
    // 有「还有上次未发布的视频，是否继续编辑？」弹窗时必须先点「放弃」，否则页面被遮挡
    await sleep(1500);
    const abandoned = await clickByText(page, ['放弃'], { selector: 'div,button,span' });
    if (abandoned) await sleep(2000);
    await page.setFile('input[type=file]', req.file);
    await page.waitFor('(function(){var t=document.body.innerText||"";'
      + 'return (t.indexOf("上传成功")>=0||t.indexOf("重新上传")>=0||!!document.querySelector("div[class*=_description_]"))&&t.indexOf("拖拽视频到此")<0;})()', 900000, 2500);
    await sleep(4000);
    await clickByText(page, ['知道了', '我知道了']);
    const descFull = String(req.title || '') + NL + '网盘：' + String(req.shareUrl || '') + ' 提取码：' + String(req.sharePasscode || '');
    await fillText(page, 'div[class*=_description_], div[contenteditable=true]', truncateBytes(descFull.replace(/[\r\n]+/g, ' '), 300));
    await sleep(800);
    // 快手发布按钮是 DIV._button_3a3lq_1._button-primary_3a3lq_60（文本「发布」），不是 <button>
    const clicked = await page.evaluate('(function(){'
      + 'var all=document.querySelectorAll("button,div[role=button],div[class*=button]");'
      + 'var cand=[];'
      + 'for(var i=0;i<all.length;i++){var e=all[i];var t=(e.innerText||"").trim();'
      + ' if(t!=="发布")continue; if(e.disabled)continue; if(!e.getClientRects().length)continue;'
      + ' if(e.querySelector&&e.querySelector("div[class*=button]"))continue;'
      + ' cand.push(e);}'
      + 'if(!cand.length)return null;var b=cand[cand.length-1];b.scrollIntoView({block:"center"});b.click();return "发布";})()');
    if (!clicked) return { status: 'failed', detail: '未找到发布按钮' };
    await sleep(1200);
    await clickByText(page, ['确定', '确认发布', '立即发布'], { selector: '.ant-modal button, .ant-modal div[class*=button], div[class*=modal] div[class*=button]' });
    const done = await page.waitFor('(function(){var t=document.body.innerText||"";'
      + 'if(t.indexOf("发布成功")>=0||t.indexOf("作品发布成功")>=0)return "ok";'
      + 'if(location.href.indexOf("article/manage")>=0||location.href.indexOf("publish/success")>=0)return "ok";'
      + 'return false;})()', 180000, 1500).catch(function () { return null; });
    const body = await bodyText(page, 300);
    if (!done) return { status: 'failed', detail: '点击' + clicked + '后未捕获成功提示 | ' + body.slice(0, 220) };
    return { status: 'ok', detail: '快手已发布 | ' + body.slice(0, 180) };
  } finally { await closePage(browser, targetId); }
}

// ---------------- x (twitter) ----------------
async function xpost(req, browser) {
  const { page, targetId } = await newPage(browser, 'https://x.com/compose/post');
  try {
    await page.waitFor('!!document.querySelector("[data-testid=tweetTextbox_0], div[role=textbox]")', 90000, 1200);
    await sleep(1200);
    const lines = [];
    if (req.text) lines.push(String(req.text));
    else {
      lines.push(truncateBytes(String(req.title || req.fileName || ''), 120));
      if (req.shareUrl) lines.push('网盘：' + String(req.shareUrl) + (req.sharePasscode ? ' 提取码：' + String(req.sharePasscode) : ''));
    }
    const text = lines.join(NL);
    // 必须用真实鼠标点进输入框：仅 e.focus() 不会让 React 进入可提交状态，发送按钮会一直 disabled
    const rectJson = await page.evaluate('(function(){var e=document.querySelector("[data-testid=tweetTextbox_0]")||document.querySelector("div[role=textbox]");if(!e)return null;var r=e.getBoundingClientRect();return JSON.stringify({x:Math.round(r.left+12),y:Math.round(r.top+r.height/2)});})()');
    if (!rectJson) return { status: 'failed', detail: '找不到推文输入框' };
    const rect = JSON.parse(rectJson);
    await page.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: rect.x, y: rect.y, button: 'left', clickCount: 1 });
    await page.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: rect.x, y: rect.y, button: 'left', clickCount: 1 });
    await sleep(400);
    await page.send('Input.insertText', { text });
    await sleep(1200);
    // tweetButton 是弹层里的提交按钮（可用）；tweetButtonInline 在同页可能一直是 disabled，不能只认它
    const clicked = await page.evaluate('(function(){'
      + 'var list=[document.querySelector("[data-testid=tweetButton]"),document.querySelector("[data-testid=tweetButtonInline]")];'
      + 'for(var i=0;i<list.length;i++){var b=list[i];if(!b)continue;if(b.disabled||b.getAttribute("aria-disabled")==="true")continue;b.click();return "clicked";}'
      + 'return list[0]||list[1]?"disabled":"null";})()');
    if (clicked !== 'clicked') return { status: 'failed', detail: '发送按钮不可用: ' + String(clicked) };
    const done = await page.waitFor('(function(){var t=document.body.innerText||"";'
      + 'if(t.indexOf("Your post was sent")>=0||t.indexOf("你的帖子已发送")>=0||t.indexOf("已发送")>=0)return "ok";'
      + 'if(location.href==="https://x.com/home")return "ok";'
      + 'return false;})()', 90000, 1200).catch(function () { return null; });
    await sleep(1500);
    const url = await page.evaluate('(function(){var a=Array.from(document.querySelectorAll("a[href*=\\"/status/\\"]")).map(function(e){return e.href;});for(var i=0;i<a.length;i++){if(a[i].indexOf("/status/")>0)return a[i];}return null;})()').catch(function () { return null; });
    if (!done) return { status: 'failed', detail: '未捕获发送成功提示', url: url };
    return { status: 'ok', detail: 'X 已发推', url: url };
  } finally { await closePage(browser, targetId); }
}

// ---------------- youtube ----------------
const YT_UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36';

async function youtube(req, browser) {
  const { page, targetId } = await newPage(browser, 'about:blank');
  try {
    // headless 默认 UA 会被 studio 判为不支持的浏览器，必须覆写
    await page.send('Network.setUserAgentOverride', { userAgent: YT_UA, acceptLanguage: 'zh-CN,zh;q=0.9,en;q=0.8', platform: 'Linux x86_64' });
    await page.goto('https://studio.youtube.com/channel/UCCj5P1tds-rFf-7lftmFS4A/videos/upload', 12000);
    // 注意：页面上的 #dialog 容器始终存在，不能用它判断是否已进入上传对话框，只能看是否有 file input
    if (await page.evaluate('!!document.querySelector("input[type=file]")')) {
      // 已在上传对话框
    } else {
      await page.waitFor('(function(){var t=document.body.innerText||"";return t.indexOf("Channel content")>=0||t.indexOf("Create")>=0;})()', 90000, 1500);
      await page.evaluate('(function(){var els=Array.from(document.querySelectorAll("ytcp-button,button,a,div"));for(var i=0;i<els.length;i++){var e=els[i];if((e.innerText||"").trim()!=="Create")continue;if(!e.getClientRects().length)continue;e.click();return 1;}return 0;})()');
      await sleep(2500);
      await page.evaluate('(function(){var els=Array.from(document.querySelectorAll("tp-yt-paper-item,div,span"));for(var i=0;i<els.length;i++){var e=els[i];if((e.innerText||"").trim()!=="Upload videos")continue;if(!e.getClientRects().length)continue;e.click();return 1;}return 0;})()');
      await sleep(3500);
    }
    await page.waitFor('!!document.querySelector("input[type=file]")', 90000, 1500);
    await page.setFile('input[type=file]', req.file);
    // 上传完成：对话框出现 "Saved as private"/"已保存为不公开"
    await page.waitFor('(function(){var d=document.querySelector("ytcp-uploads-dialog");if(!d)return false;var t=String(d.innerText||"");'
      + 'return t.indexOf("Saved as private")>=0||t.indexOf("已保存为不公开")>=0||t.indexOf("Upload complete")>=0;})()', 1800000, 3000);
    await sleep(1200);
    // 标题
    const title = truncateBytes(req.title || req.fileName || 'video', 95);
    const setContentEditable = async (sel, text) => {
      const ok = await page.evaluate('(function(){var e=document.querySelector(' + JSON.stringify(sel) + ');if(!e)return false;e.focus();'
        + 'try{var r=document.createRange();r.selectNodeContents(e);var s=getSelection();s.removeAllRanges();s.addRange(r);}catch(x){}return true;})()');
      if (!ok) return false;
      await page.send('Input.insertText', { text });
      await sleep(400);
      return true;
    };
    await setContentEditable('ytcp-uploads-dialog #title-textarea #textbox', title);
    const descFull = String(req.title || '') + NL + '网盘：' + String(req.shareUrl || '') + ' 提取码：' + String(req.sharePasscode || '');
    await setContentEditable('ytcp-uploads-dialog [aria-label*="Tell viewers"], ytcp-uploads-dialog [aria-label*="向观众"]', truncateBytes(descFull.replace(/[\r\n]+/g, ' '), 4000));
    // 逐页 Next：Details -> Video elements -> Checks(可能耗时数分钟) -> Visibility
    const isVisibility = '(function(){var d=document.querySelector("ytcp-uploads-dialog");if(!d)return false;'
      + 'var r=Array.from(d.querySelectorAll("tp-yt-paper-radio-button")).map(function(e){return e.name||"";});'
      + 'return r.indexOf("VIDEO_PUBLIC")>=0||r.join(",").indexOf("PUBLIC")>=0;})()';
    const t0 = Date.now();
    let reached = false;
    while (Date.now() - t0 < 900000) {
      if (await page.evaluate(isVisibility)) { reached = true; break; }
      // 有必答的「是否为儿童打造」未选时先答 No
      await page.evaluate('(function(){var rs=Array.from(document.querySelectorAll("tp-yt-paper-radio-button"));'
        + 'for(var i=0;i<rs.length;i++){if(rs[i].name==="VIDEO_MADE_FOR_KIDS_NOT_MFK"&&rs[i].getAttribute("aria-checked")!=="true"){'
        + 'var t=rs[i].querySelector("#radioContainer")||rs[i].querySelector("div")||rs[i];t.click();return 1;}}return 0;})()');
      const st = await page.evaluate('(function(){var b=document.querySelector("#next-button");return b?(b.disabled?"disabled":"enabled"):"none";})()');
      if (st === 'enabled') { await page.evaluate('document.querySelector("#next-button").click()'); }
      await sleep(3500);
    }
    if (!reached) {
      const tail = await page.evaluate('(function(){var d=document.querySelector("ytcp-uploads-dialog");var t=d?String(d.innerText||""):String(document.body.innerText||"");'
        + 'return t.split(String.fromCharCode(10)).join(" | ").slice(-260);})()');
      return { status: 'failed', detail: '未能进入可见性步骤（校验可能超时）| ' + String(tail).slice(0, 220) };
    }
    // 选择可见性：public / unlisted / private
    const want = String(req.youtubeVisibility || 'public');
    const label = want === 'private' ? 'Private' : (want === 'unlisted' ? 'Unlisted' : 'Public');
    const picked = await page.evaluate('(function(){var want=' + JSON.stringify(label) + ';'
      + 'var rs=Array.from(document.querySelectorAll("ytcp-uploads-dialog tp-yt-paper-radio-button"));'
      + 'for(var i=0;i<rs.length;i++){var e=rs[i];var t=(e.innerText||"").trim().toLowerCase();'
      + ' if(t.indexOf(want.toLowerCase())!==0)continue;'
      + ' var c=e.querySelector("#radioContainer")||e.querySelector("div")||e;c.click();return (e.innerText||"").trim().slice(0,20);}'
      + 'return null;})()');
    await sleep(1500);
    const saveClicked = await page.evaluate('(function(){var b=document.querySelector("#done-button");if(!b)return "none";if(b.disabled)return "disabled";'
      + 'try{var sr=b.shadowRoot;}catch(e){}b.click();return "clicked";})()');
    if (saveClicked !== 'clicked') return { status: 'failed', detail: '保存按钮不可用: ' + String(saveClicked) + ' 可见性=' + String(picked) };
    // 成功后会出现确认对话框（Published / 已发布），视频链接 https://youtu.be/xxxx
    const done = await page.waitFor('(function(){var t=document.body.innerText||"";var l=Array.from(document.querySelectorAll("a")).map(function(e){return e.href||"";});'
      + 'var yt=l.filter(function(h){return h.indexOf("youtu.be/")>0;});'
      + 'if(yt.length&&(t.indexOf("Video published")>=0||t.indexOf("已发布")>=0||t.indexOf("链接")>=0||t.indexOf("Published")>=0))return yt[0];'
      + 'return false;})()', 240000, 2000).catch(function () { return null; });
    const url = done || await page.evaluate('(function(){var l=Array.from(document.querySelectorAll("a")).map(function(e){return e.href||"";});var yt=l.filter(function(h){return h.indexOf("youtu.be/")>0;});return yt.length?yt[0]:null;})()').catch(function () { return null; });
    if (!done) return { status: 'failed', detail: '点击保存后未捕获发布确认', url: url };
    // 关闭确认对话框（隐藏的 secondary-action-button 需要 DOM click）
    await page.evaluate('(function(){var b=document.querySelector("#secondary-action-button button")||document.querySelector("#close-button");if(b){b.click();return 1;}return 0;})()').catch(function () { return null; });
    return { status: 'ok', detail: 'YouTube 已发布(' + label + ')', url: url };
  } finally { await closePage(browser, targetId); }
}

const HANDLERS = { bilibili, douyin, xhs, kuaishou, x: xpost, youtube };

export async function publishPlatform(payload) {
  const dest = String(payload.dest || '');
  const fn = HANDLERS[dest];
  if (!fn) return { status: 'unavailable', detail: '暂不支持平台: ' + dest };
  let browser = null;
  try {
    browser = await connectBrowser(payload.cdpUrl);
    const r = await fn(payload, browser);
    return { status: r.status, detail: String(r.detail || '').slice(0, 500), url: r.url || null };
  } catch (e) {
    return { status: 'failed', detail: ('平台提交异常: ' + String((e && e.message) || e)).slice(0, 500) };
  } finally { try { if (browser) browser.close(); } catch (e) {} }
}

// 作为命令行使用时：stdin JSON -> stdout 一行 JSON
if (process.argv[1] && process.argv[1].endsWith('platform-publish.mjs')) {
  let input = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (c) => { input += c; });
  process.stdin.on('end', async () => {
    let payload = {};
    try { payload = JSON.parse(input); } catch (e) { console.log(JSON.stringify({ status: 'failed', detail: '输入不是 JSON' })); process.exit(0); }
    const r = await publishPlatform(payload);
    console.log(JSON.stringify(r));
  });
}
