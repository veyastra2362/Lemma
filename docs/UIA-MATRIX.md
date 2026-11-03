# 取词能力矩阵（实测）

> 这份文件只记录**实测结果**，不写「理论上应该」。方法：`trans.exe --probe`。
> 环境：Windows 11 / Chrome 156.0.8078.80 / Rust 1.99.1。

## 怎么自己测

```powershell
# 把鼠标停在目标应用的文字上，然后：
.\src-tauri\target\release\trans.exe --probe

# 或指定屏幕坐标 / 延迟（切窗口后让目标应用稳住）
.\src-tauri\target\release\trans.exe --probe --point 800,400 --delay 800
```

输出同时落到 `%APPDATA%\dev.lunar.trans\probe\last.json`（机器可读，可攒成矩阵）
和 `last.txt`（人读）。JSON 里的 `patterns` 字段就是这个控件支持的 UIA Pattern 清单。

## 实测结果

| 应用 | 控件类名 | 支持的 Pattern | T1 选区 | T0 光标词 | 结论 |
| --- | --- | --- | --- | --- | --- |
| **记事本**（Win11，富文本） | `RichEditD2DPT` | `Text, Text2, Value, LegacyIAccessible` | 支持（空选区时返回空串） | **命中**：`threshold`，33.9 ms | 原生控件走 UIA 完全可用，句子/段落都能拿到 |
| **Chrome 156**（页面正文） | `View` | **仅 `LegacyIAccessible`** | ✗ | ✗ | 没有 TextPattern，UIA 这条路走不通 |
| **Chrome 156 + `--enable-features=UiaProvider`** | `Chrome_WidgetWin_1` | 仅 `LegacyIAccessible` | ✗ | ✗ | 仍拿不到（无障碍树未就绪 / 该特性尚未覆盖） |
| Chromium 系（Electron / WebView2，含本应用自己的窗口） | `View` | 仅 `LegacyIAccessible` | ✗ | ✗ | 与 Chrome 相同 |

### 关键结论

1. **我们的 UIA 实现是对的。** 记事本上一次就命中了词、句子、段落，耗时 33.9 ms。
2. **瓶颈在 Chromium，不在我们。** Chromium 原生 UIA provider 在 M120 的官方文档里还写着
   「under development，需要用 `--enable-features=UiaProvider` 开启」
   （[Chromium UIA 文档](https://chromium.googlesource.com/chromium/src/+/refs/tags/120.0.6099.43/docs/accessibility/browser/uiautomation.md)）。
   实测即使带上这个开关也没有拿到 `Text` pattern，所以**不能把浏览器的取词寄托在 UIA TextPattern 上**。
3. 因此浏览器只能走另外两条路：**T2 剪贴板（Ctrl+C）** 或 **MSAA / IAccessible2**（IA2）。
4. 顺带验证：`LegacyIAccessible` 这一档在所有 Chromium 应用上都有，但它是 MSAA 桥，
   只有 `accValue` / `accName` 这类粗粒度信息，**给不了「光标下的词」**。

## 对 M1 的影响（已定）

| 档 | 覆盖 | 状态 |
| --- | --- | --- |
| T1 选区直读（UIA TextPattern） | 原生 Win32 / WinForms / WPF / WinUI / 记事本 | ✅ 已验证 |
| T0 光标取词（UIA RangeFromPoint） | 同上 | ✅ 已验证 |
| **T2 剪贴板兜底（模拟 Ctrl+C）** | **Chrome / Firefox / Electron / PDF / 几乎一切可复制的应用** | ⬅ 下一步就做这个，浏览器靠它 |
| T3 IAccessible2（MSAA 直读） | 浏览器 / 部分自绘界面的「不污染剪贴板」快路径 | 排在 T2 之后，作为体验优化 |

> 为什么先做 T2 而不是直接上 IA2：T2 一天能落地且覆盖全部软件，IA2 需要手写一整套
> COM 虚表（`IAccessible2` / `ITextStoreACP` 不在 `windows` crate 里），
> 工作量大且只能优化 Chrome / Firefox 两个应用。先把能天天用的闭环跑起来。

## 附：Chrome 的无障碍触发条件（备忘）

- Chromium 的无障碍树本身是按需开启的；`UIA` 客户端访问**不一定**能触发它。
- 原生 UIA provider 是独立特性：`--enable-features=UiaProvider`（截至 M120 仍是开发中）。
- 目前实测：**默认配置的 Chrome 不给 TextPattern**。所以不要在这上面浪费更多时间。

---

# 待补充（还没测）

- [ ] Firefox（IA2 主要支持者，UIA 是桥接的）
- [ ] VS Code / Electron（预期与 Chrome 同）
- [ ] Word / WPS（预期有 TextPattern）
- [ ] PDF 阅读器（Adobe / Edge 内置 / SumatraPDF）
- [ ] 终端（Windows Terminal / conhost）
- [ ] 输入框类控件（地址栏、聊天框）的 T1 行为
