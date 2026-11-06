// Lemma 回填 —— content script
//
// 为什么回填要在页面里做：浏览器不把网页内容交给外部程序改
// （UIA 的 ITextRangeProvider 没有 SetText，文档的 ValuePattern 只读）。
// 只有页面内的脚本能改 DOM，所以这一步必须由扩展完成。
//
// 三类目标，按可靠性排序：
//   1. <textarea> / <input>：setRangeText + 派发 input 事件（受控组件才不会被弹回去）
//   2. contenteditable：execCommand("insertText")，唯一能进浏览器原生撤销栈的方式
//   3. 普通文本 Range：deleteContents + insertNode（覆盖绝大多数文章页）

/** 保存上次选中范围，供「回填」使用（Range 不随窗口失焦失效）。 */
let lastRange = null;
/** 保存替换前的原文，用于撤回。 */
let lastUndo = null;

function currentSelectionInfo() {
  const active = document.activeElement;

  // ---- 表单控件 ----
  if (active instanceof HTMLTextAreaElement || active instanceof HTMLInputElement) {
    const start = active.selectionStart ?? 0;
    const end = active.selectionEnd ?? 0;
    const text = active.value.slice(start, end);
    if (text.trim()) {
      return { kind: "field", element: active, start, end, text };
    }
  }

  // ---- 页面选区（含 contenteditable）----
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
    return null;
  }
  const range = selection.getRangeAt(0);
  const text = selection.toString();
  if (!text.trim()) {
    return null;
  }
  const editable = range.commonAncestorContainer.parentElement?.isContentEditable ?? false;
  return { kind: editable ? "editable" : "range", range, text };
}

/** 把译文写回去。 */
function applyText(info, replacement) {
  if (info.kind === "field") {
    const element = info.element;
    const before = element.value;
    element.focus();
    element.setRangeText(replacement, info.start, info.end, "end");
    // 受控组件（Vue/React）要靠 input 事件才认账
    element.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: "insertReplacementText",
        data: replacement,
      }),
    );
    lastUndo = { kind: "field", element, before, start: info.start, end: info.start + replacement.length };
    return true;
  }

  if (info.kind === "editable") {
    // execCommand 会进原生撤销栈：用户按 Ctrl+Z 就能还原
    info.range.deleteContents();
    info.range.collapse(true);
    const node = document.createTextNode(replacement);
    info.range.insertNode(node);
    const after = document.createRange();
    after.selectNodeContents(node);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(after);
    lastUndo = { kind: "editable", node, original: info.text };
    return true;
  }

  // 普通 Range：先记下上下文，替换后能反向找回
  const range = info.range;
  const parent = range.commonAncestorContainer;
  const original = info.text;
  range.deleteContents();
  const node = document.createTextNode(replacement);
  range.insertNode(node);
  const after = document.createRange();
  after.selectNodeContents(node);
  const selection = window.getSelection();
  selection.removeAllRanges();
  selection.addRange(after);
  lastUndo = { kind: "range", node, parent, original };
  return true;
}

/** 撤回上一次回填。 */
function undo() {
  if (!lastUndo) {
    return false;
  }
  const undoInfo = lastUndo;
  lastUndo = null;

  if (undoInfo.kind === "field") {
    const element = undoInfo.element;
    element.focus();
    element.value = undoInfo.before;
    element.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "historyUndo" }));
    return true;
  }

  const node = undoInfo.node;
  if (!node?.isConnected) {
    return false;
  }
  const replacement = document.createTextNode(undoInfo.original);
  node.replaceWith(replacement);
  return true;
}

/** 页面角上的小提示。 */
function toast(text, kind) {
  const id = "lemma-toast";
  document.getElementById(id)?.remove();
  const box = document.createElement("div");
  box.id = id;
  box.textContent = text;
  box.style.cssText = [
    "position:fixed",
    "right:16px",
    "bottom:16px",
    "z-index:2147483647",
    "padding:8px 14px",
    "border-radius:8px",
    "font:13px/1.5 system-ui,-apple-system,'Segoe UI',sans-serif",
    "color:#eeeae1",
    "background:#242321",
    "border:1px solid #46423d",
    "box-shadow:0 8px 22px rgba(0,0,0,.38)",
    "pointer-events:none",
    "opacity:0",
    "transition:opacity .12s ease-out",
  ].join(";");
  if (kind === "error") {
    box.style.borderColor = "#8c4a44";
  } else if (kind === "ok") {
    box.style.borderColor = "#bd7454";
  }
  document.documentElement.appendChild(box);
  requestAnimationFrame(() => {
    box.style.opacity = "1";
  });
  window.setTimeout(() => {
    box.style.opacity = "0";
    window.setTimeout(() => box.remove(), 200);
  }, kind === "busy" ? 1500 : 2200);
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "lemma:pick") {
    const info = currentSelectionInfo();
    if (!info) {
      sendResponse({ ok: false, error: "请先选中要翻译的文本" });
      return true;
    }
    if (info.kind === "range" || info.kind === "editable") {
      lastRange = info.range.cloneRange();
    }
    // 把可序列化的部分留给后续步骤：Range 与元素都存在闭包外的变量里
    pending = info;
    sendResponse({ ok: true, text: info.text });
    return true;
  }

  if (message?.type === "lemma:apply") {
    if (!pending) {
      sendResponse({ ok: false, error: "选区已失效，请重新选中" });
      return true;
    }
    try {
      const done = applyText(pending, message.text);
      pending = null;
      sendResponse({ ok: done });
    } catch (error) {
      sendResponse({ ok: false, error: String(error?.message ?? error) });
    }
    return true;
  }

  if (message?.type === "lemma:undo") {
    sendResponse({ ok: undo() });
    return true;
  }

  if (message?.type === "lemma:toast") {
    toast(message.text, message.kind);
    sendResponse({ ok: true });
    return true;
  }

  return false;
});

/** 上一次 pick 的可写目标（Range / 表单元素），只在本次页面生命周期内有效。 */
let pending = null;
