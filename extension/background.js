// Lemma 回填 —— background service worker
//
// 职责：
//   1. 与本地 Lemma 应用通信（Native Messaging，宿主名 `dev.lunar.lemma`）
//   2. 把「翻译并回填」的请求转给 content script
//
// 协议见 docs/EXTENSION.md。宿主侧实现在 src-tauri/src/native_host.rs：
// stdio 上是「4 字节小端长度 + UTF-8 JSON」，Chrome/Firefox 会替我们做这层封装。

const HOST = "dev.lunar.lemma";

/** 与宿主的一次往返。 */
function callHost(message) {
  return new Promise((resolve, reject) => {
    let port;
    try {
      port = chrome.runtime.connectNative(HOST);
    } catch (error) {
      reject(new Error(`连不上 Lemma（${error.message}）。桌面应用装了吗？`));
      return;
    }

    let settled = false;
    const finish = (fn, value) => {
      if (settled) {
        return;
      }
      settled = true;
      try {
        port.disconnect();
      } catch {
        /* 已经断了就算了 */
      }
      fn(value);
    };

    port.onMessage.addListener((response) => {
      if (response && response.ok) {
        finish(resolve, response);
      } else {
        finish(reject, new Error(response?.error || "宿主返回了失败"));
      }
    });

    port.onDisconnect.addListener(() => {
      const detail = chrome.runtime.lastError?.message ?? "宿主已断开";
      finish(reject, new Error(detail));
    });

    port.postMessage(message);
  });
}

/** 让某个标签页的 content script 做事。 */
async function askTab(tabId, message) {
  // 页面刷新后 content script 会没；先 ping 一下，失败就注入
  try {
    return await chrome.tabs.sendMessage(tabId, message);
  } catch {
    await chrome.scripting.executeScript({ target: { tabId }, files: ["content.js"] });
    return await chrome.tabs.sendMessage(tabId, message);
  }
}

/** 「翻译并回填」主流程。 */
async function translateAndReplace(tabId, frameId) {
  const picked = await askTab(tabId, { type: "lemma:pick" });
  if (!picked?.ok) {
    return { ok: false, error: picked?.error ?? "没有选中的文本" };
  }

  const translated = await callHost({ method: "translate", text: picked.text });
  const applied = await askTab(tabId, {
    type: "lemma:apply",
    text: translated.text,
  });
  if (!applied?.ok) {
    return { ok: false, error: applied?.error ?? "替换失败" };
  }
  return { ok: true, original: picked.text, text: translated.text };
}

/** 往页面角上弹一句提示（content script 负责画）。 */
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
    const result = await translateAndReplace(tab.id, 0);
    toast(tab.id, result.ok ? "已回填" : `失败：${result.error}`, result.ok ? "ok" : "error");
  } catch (error) {
    toast(tab.id, `失败：${error.message}`, "error");
  }
});

// 右键菜单：选中文本 →「用 Lemma 翻译并回填」
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "lemma-replace",
    title: "用 Lemma 翻译并回填",
    contexts: ["selection"],
  });
});

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== "lemma-replace" || !tab?.id) {
    return;
  }
  toast(tab.id, "翻译中…", "busy");
  try {
    const result = await translateAndReplace(tab.id, 0);
    toast(tab.id, result.ok ? "已回填" : `失败：${result.error}`, result.ok ? "ok" : "error");
  } catch (error) {
    toast(tab.id, `失败：${error.message}`, "error");
  }
});
