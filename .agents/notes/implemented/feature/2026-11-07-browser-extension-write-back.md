# Agent Note: 浏览器扩展回填（取代原生回填）

Status: implemented

## 问题

用户要在网页里「就地读译文」：选中一句 → 译文直接替换原文。
桌面应用做不到——这不是实现程度问题，而是平台限制（见下）。

## 决定

回填**完全交给浏览器扩展**，应用只做两件事：查词/翻译，以及在 HUD 上提供一个「回填」入口。

```
选中 → 双击 Ctrl → 应用用 UIA 取词 → HUD 显示译文
      → 点「回填」→ 应用写命名管道 → 宿主进程 → 帧 → 扩展 → content script 替换 DOM
```

三段实现：

| 段 | 位置 | 说明 |
| --- | --- | --- |
| 协议 | `src-tauri/src/native_host.rs` | `--native-host`，stdio 上 4 字节长度 + JSON |
| 反向通道 | `src/platform/windows/pipe.rs` + `core/writeback.rs` | 宿主建 `\\.\pipe\lemma-writeback`，GUI 每次点按钮连一次 |
| DOM 替换 | `extension/content.js` | 普通 Range / 表单控件 / contenteditable 三类 |

## 依据（实测）

| 目标 | UIA 能看到 | 能写 |
| --- | --- | --- |
| 记事本 | RichEdit 文档，Value 可写 | ✅ |
| Chrome | 文档，Value **只读** | ❌ |
| Firefox | 只有顶层窗口 | ❌ |

`ITextRangeProvider` 没有 SetText，所以从进程外改网页 DOM 不可能；唯一路径是模拟输入，
而实测会出现「焦点抢不到 → 译文粘到别的应用」和剪贴板污染。用户明确否掉。

## 备选方案

**原生 `EM_REPLACESEL`**（已实现过，已删除）。对记事本这类控件完美，但浏览器用不了，
而浏览器是主要阅读场景。

**剪贴板 + 模拟 Ctrl+V**（已实现过，已删除）。见上：粘错窗口、污染剪贴板。

**只做只读提示（浮层贴附）**。零风险，但不是「就地读译文」，用户不接受。

## 后果

- 需要用户装一次扩展（浏览器不允许程序代装），应用设置里给「获取扩展（GitHub）」
  + 自动写宿主清单与注册表；扩展包由 GitHub Release 分发。
- 扩展 ID 由 manifest 的 `key` 固定为 `kchhodaiilomkholhbpajlgkblnikfpb`，
  宿主清单白名单写死，用户不必手填 ID。
- **常驻代价**：宿主进程 idle 工作集 11.9 MB / 私有 2.8 MB（见 `docs/PERF.md` 3.6）。
- 扩展用 `selectionchange` 记住最近选区（clone 一个 Range，成本可忽略），
  这样应用点「回填」时扩展知道该改哪里。
- 设置里两个开关：`extension_write_back`（关掉后宿主拒绝扩展请求）、
  `hud_write_back`（关掉后 HUD 不显示回填按钮）。
- 网页里另有右键菜单与 `Alt+Shift+L` 两条不需要 HUD 的入口。
