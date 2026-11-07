// Lemma 回填 —— background service worker
//
// 两条链路：
//   A. 扩展主动：右键菜单 / 快捷键 → 让宿主翻译 → 自己替换页面
//   B. 应用主动：HUD 上点「回填」→ 应用写命名管道 → 宿主推帧过来 → 这里派给页面
//
// 为了 B，必须保持一条**常驻**的 connectNative 连接（Chrome 允许宿主在连接存活期间
// 持续发消息；顺便让 MV3 的 service worker 不被回收）。断线后按退避重连。

const HOST = "dev.lunar.lemma";

let port = null;
let retry = 0;

function connect() {
  if (port) {
    return port;
  }
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch (error) {
    scheduleReconnect();
    return null;
  }

  port.onMessage.addListener((message) => {
    // 应用推过来的「替换这段文本」
    if (message?.type === "replace") {
      applyFromApp(message.id, message.text);
      return;
    }
    pendingByRequest.get(message?.requestId)?.(message);
  });

  port.onDisconnect.addListener(() => {
    port = null;
    const detail = chrome.runtime.lastError?.message;
    if (detail) {
      console.warn("[Lemma] 宿主断开：", detail);
    }
    scheduleReconnect();
  });

  retry = 0;
  return port;
}

function scheduleReconnect() {
  retry = Math.min(retry + 1, 6);
  // 1s、2s、4s…最多 32s，避免宿主没装好时把 CPU 烧了
  const delay = Math.min(1000 * 2 ** (retry - 1), 32000);
  setTimeout(connect, delay);
}

/** 与宿主的一次性请求（ping / translate）。 */
const pendingByRequest = new Map();
let nextRequestId = 1;

function callHost(message) {
  return new Promise((resolve, reject) => {
    const active = connect();
    if (!active) {
      reject(new Error("连不上 Lemma（桌面应用装了吗？）"));
      return;
    }
    const requestId = nextRequestId++;
    pendingByRequest.set(requestId, (response) => {
      pendingByRequest.delete(requestId);
      if (response?.ok) {
        resolve(response);
      } else {
        reject(new Error(response?.error || "宿主返回失败"));
      }
    });
    active.postMessage({ ...message, requestId });

    // 兜底：10 秒没有回话就当失败（翻译走网络，留足时间）
    setTimeout(() => {
      if (pendingByRequest.delete(requestId)) {
        reject(new Error("宿主没有响应"));
      }
    }, 10000);
  });
}

/** 应用推送来的替换请求：交给当前标签页的 content script。 */
async function applyFromApp(id, text) {
  let reply = { id, ok: false, error: "没有活动标签页" };
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id) {
      throw new Error("没有活动标签页");
    }
    const applied = await askTab(tab.id, { type: "lemma:apply-last", text });
    reply = applied?.ok
      ? { id, ok: true }
      : { id, ok: false, error: applied?.error ?? "替换失败" };
  } catch (error) {
    reply = { id, ok: false, error: String(error?.message ?? error) };
  }
  // 回给宿主（宿主再通过管道回给应用）
  try {
    connect()?.postMessage(reply);
  } catch {
    /* 宿主没了就算了 */
  }
}

async function askTab(tabId, message) {
  try {
    return await chrome.tabs.sendMessage(tabId, message);
  } catch {
    await chrome.scripting.executeScript({ target: { tabId }, files: ["content.js"] });
    return await chrome.tabs.sendMessage(tabId, message);
  }
}

async function translateAndReplace(tabId) {
  const picked = await askTab(tabId, { type: "lemma:pick" });
  if (!picked?.ok) {
    return { ok: false, error: picked?.error ?? "没有选中的文本" };
  }
  const translated = await callHost({ method: "translate", text: picked.text });
  const applied = await askTab(tabId, { type: "lemma:apply", text: translated.text });
  if (!applied?.ok) {
    return { ok: false, error: applied?.error ?? "替换失败" };
  }
  return { ok: true };
}

function toast(tabId, text, kind) {
  chrome.tabs.sendMessage(tabId, { type: "lemma:toast", text, kind }).catch(() => {});
}

chrome.commands.onCommand.addListener(async (command) => {
  if (command !== "translate-selection") {
    return;
  }
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id) {
    return;
  }
  toast(tab.id, "翻译中…", "busy");
  try {
    const result = await translateAndReplace(tab.id);
    toast(tab.id, result.ok ? "已回填" : `失败：${result.error}`, result.ok ? "ok" : "error");
  } catch (error) {
    toast(tab.id, `失败：${error.message}`, "error");
  }
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "lemma-replace",
    title: "用 Lemma 翻译并回填",
    contexts: ["selection"],
  });
  connect();
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== "lemma-replace" || !tab?.id) {
    return;
  }
  toast(tab.id, "翻译中…", "busy");
  try {
    const result = await translateAndReplace(tab.id);
    toast(tab.id, result.ok ? "已回填" : `失败：${result.error}`, result.ok ? "ok" : "error");
  } catch (error) {
    toast(tab.id, `失败：${error.message}`, "error");
  }
});

// service worker 被唤醒时把连接补上
chrome.runtime.onStartup.addListener(connect);
connect();
