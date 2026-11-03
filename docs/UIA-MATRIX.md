# 取词能力矩阵（实测）

> 这份文件只记录**实测结果**，不写「理论上应该」。方法：`lemma.exe --probe`。
> 环境：Windows 11 / Chrome 156.0.8078.80 / Rust 1.99.1。

## 怎么自己测

```powershell
# 把鼠标停在目标应用的文字上，然后：
.\src-tauri\target\release\lemma.exe --probe

# 或指定屏幕坐标 / 延迟（切窗口后让目标应用稳住）
.\src-tauri\target\release\lemma.exe --probe --point 800,400 --delay 800
```

输出同时落到 `%APPDATA%\dev.lunar.lemma\probe\last.json`（机器可读，可攒成矩阵）
和 `last.txt`（人读）。JSON 里的 `patterns` 字段就是这个控件支持的 UIA Pattern 清单。

## 实测结果

| 应用 | 控件类名 | 支持的 Pattern | T1 选区 | T0 光标词 | 结论 |
| --- | --- | --- | --- | --- | --- |
| **记事本**（Win11，富文本） | `RichEditD2DPT` | `Text, Text2, Value, LegacyIAccessible` | 支持（空选区时返回空串） | **命中**：`threshold`，33.9 ms | 原生控件走 UIA 完全可用，句子/段落都能拿到 |
| **Chrome 156**（页面正文根） | `Chrome_RenderWidgetHostHWND` | `Text, Text2, Value, LegacyIAccessible` | **✅ 手测通过，8 ms** | 时有时无（拿到过 `第`，也拿到过空） | **T1 可靠、T0 不可靠** |
| **Chrome 156**（正文内层元素） | `View` | **仅 `LegacyIAccessible`** | ✗ | ✗ | 内层节点没有 TextPattern |
| **Firefox** | `MozillaWindowClass` | **仅 `Value, LegacyIAccessible`** | ✗ | ✗ | 没有 TextPattern，只能走 T2 |
| Chromium 系（Electron / WebView2） | `View` / `Chrome_WidgetWin_1` | 仅 `LegacyIAccessible` | ✗ | ✗ | 与 Chrome 内层一致 |

### 关键结论

1. **我们的 UIA 实现是对的。** 记事本上一次就命中了词、句子、段落，耗时 33.9 ms。
2. **Chromium：T1 可靠，T0 不可靠。** 这是一个必须区分对待的结论：
   - **T1（焦点元素 + `GetSelection`）实测可用**：2026-11-03 手动验证，Chrome 156 选中一段网页文字后
     双击 Ctrl，HUD 正确显示原文，**耗时 8 ms，档位标「选区」——完全没有碰剪贴板**。
   - **T0（`RangeFromPoint` + `ExpandToEnclosingUnit(Word)`）不稳定**：同一个 Chrome 进程，
     pattern 三次都在，但展开经常返回空串；应用里也真的抓到过 `第` 这种单字。

   **产品结论**：浏览器场景的正确用法是「**选中 → 双击**」，走 8 ms 的 T1 快路径；
   「悬停取词」在浏览器上不可靠，剪贴板兜底只在 T1 也拿不到时兜住。
   这条直接影响交互设计与文档措辞——不要在浏览器里承诺「鼠标悬停取词」。
3. **Firefox 完全没有 TextPattern**（只有 `Value` / `LegacyIAccessible`），
   和社区反馈一致：它走的是 MSAA/IAccessible2。浏览器里它的优先级排在 Chrome 之后是对的。
4. **CJK 分词不能指望 UIA。** `TextUnit_Word` 是按空白/连字符切的，
   中文会返回单个字（实测 `第`）。中文取词需要自己的分词，见下方待办。
5. `LegacyIAccessible` 在所有 Chromium/Firefox 上都有，但它是 MSAA 桥，
   只给 `accValue` / `accName` 这类粗粒度信息，**给不了「光标下的词」**。

## 一个被实测纠正的设计错误

初版 `gather()` 用「光标下的元素」去读选区（`ElementFromPoint` → `GetSelection`）。
这是错的：用户选中文字后光标可能停在别处甚至别的窗口上。
实测就直接抓到了 `app=DeepSeek Harness.exe text=这` 这种串台结果。

**正确做法**：T1 用 `GetFocusedElement()`（焦点元素）读选区，T0 才用 `ElementFromPoint`（光标）读词。
已修正，见 `platform/windows/uia.rs::gather`。

## 对 M1 的影响（已定）

| 档 | 覆盖 | 状态 |
| --- | --- | --- |
| T1 选区直读（UIA TextPattern，焦点元素） | 原生 Win32 / WinForms / WPF / WinUI / 记事本 / **Chrome** | ✅ 已验证（记事本 + Chrome 8ms） |
| T0 光标取词（UIA RangeFromPoint） | 原生控件可用；**Chromium 不稳定** | ✅ 已验证（记事本）；浏览器不承诺 |
| **T2 剪贴板兜底（模拟 Ctrl+C）** | Firefox / Electron / PDF / 拿不到 T1 时的最后兜底 | ✅ 失败路径已验证（超时报错 492ms）；⚠️ 成功路径待手测 |
| T3 IAccessible2（MSAA 直读） | Firefox 的唯一快路径；也可优化 Chrome 的 T0 | 排在 T2 之后 |

> ⚠️ **T2 成功路径尚未实测**：自动化脚本里 `SetForegroundWindow` / `AppActivate` 被
> 前台锁拦住（本机有 QQ / 微信 / electron 应用在抢焦点），没法稳定把浏览器切到前台。
> 手动验证方法：在 Chrome 里选中一段文字 → 双击 Ctrl → HUD 应显示原文且来源标「剪贴板」。

## 待补充（还没测）

- [ ] Firefox（已探针：无 TextPattern；取词走 T2 未验证）
- [ ] VS Code / Electron（预期与 Chrome 内层同）
- [ ] Word / WPS（预期有 TextPattern）
- [ ] PDF 阅读器（Adobe / Edge 内置 / SumatraPDF）
- [ ] 终端（Windows Terminal / conhost）
- [ ] 输入框类控件（地址栏、聊天框）的 T1 行为
- [ ] **CJK 分词**：UIA 的 WordUnit 对中文只返回单字，需要接一个中文分词（M2 词库那步一起做）
