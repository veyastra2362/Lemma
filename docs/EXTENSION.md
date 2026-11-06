# 浏览器扩展回填方案

> 桌面应用受 Windows 限制：UIA 的 `ITextRangeProvider` 没有 SetText，
> 浏览器文档的 `ValuePattern` 又是只读。**改网页内容只能在浏览器内部做**，
> 所以回填改由扩展完成，应用只负责查词与翻译。

## 分工

| 角色 | 职责 |
| --- | --- |
| **content script**（扩展） | 捕获选区 `Range`、替换 DOM、维护撤销 |
| **service worker**（扩展） | 与原生宿主通信、路由消息、按 tab/frame 分发 |
| **`lemma.exe --native-host`** | 查词库、调翻译、维护缓存（复用现有全部逻辑） |
| **HUD**（应用窗口） | 显示译文、提供「回填」按钮（界面不变） |

## 通信：Native Messaging

不用本地端口（防火墙弹窗、任意进程可连），用 Chrome/Firefox 官方的 Native Messaging：
**stdin/stdout 上「4 字节小端长度前缀 + JSON」**。

### 请求 / 响应

| 方法 | 请求 | 响应 |
| --- | --- | --- |
| `ping` | `{}` | `{ "ok": true, "version": "0.1.0" }` |
| `translate` | `{ "text": "...", "target": "zh" \| "en" }` | `{ "ok": true, "text": "...", "provider": "google", "cached": false }` |
| `lookup` | `{ "word": "..." }` | `{ "ok": true, "entry": { ... } \| null }` |

限制：`text` 上限 8192 字符；未知方法一律 `{ "ok": false, "error": "..." }`。

### 宿主注册

| 浏览器 | 位置 |
| --- | --- |
| Chrome / Edge | 注册表 `HKCU\Software\Google\Chrome\NativeMessagingHosts\dev.lunar.lemma` → manifest 路径 |
| Firefox | `%APPDATA%\Mozilla\NativeMessagingHosts\dev.lunar.lemma.json` |

两种 manifest 都要写 `path`（exe 绝对路径）与 `allowed_extensions` / `allowed_origins`。
**由应用内的「安装浏览器扩展」按钮自动完成**（写注册表 + 写 JSON + 复制扩展文件），
用户只需在 `chrome://extensions` 打开开发者模式并加载目录——这一步浏览器不允许程序代劳。

## DOM 替换策略（真正的工作量）

| 场景 | 做法 |
| --- | --- |
| 普通文章（`<p>` / `<span>`） | `range.deleteContents()` + 插入文本节点 |
| 跨元素选中 | 同上；注意保留块级边界，必要时按段落拆成多次替换 |
| `<textarea>` / `<input>` | `setRangeText()`，随后派发 `InputEvent{ inputType: "insertReplacementText" }`，否则受控组件会把值弹回 |
| `contenteditable`（Notion、邮件） | `document.execCommand("insertText")` —— 唯一能进浏览器**原生撤销栈**的方式 |
| 跨域 iframe | `all_frames: true`，消息带 `frameId` 路由；`about:blank` 与 `sandbox` 内放弃 |

**撤销**：替换前把 `{ range克隆, 原文 }` 存进 `Map`，`undo` 时反向替换。
`execCommand` 路径同时也能被用户自己的 Ctrl+Z 撤销。

## 交互流程

1. 用户在页面选中文本 → 按 **双击 Ctrl**（应用现有全局热键，习惯不变）
2. 应用向扩展要「当前选区」→ 扩展回 `{ text, token }`（token 是内存里的 Range 句柄）
3. 应用查词/翻译 → 弹 HUD（同一个界面）
4. 用户点「回填」→ 应用把 `{ token, text }` 发回扩展 → 扩展替换 DOM
5. 扩展回 `{ ok }`；应用在 HUD 上显示「已回填」，并把按钮换成「撤回」

**焦点问题自然消失**：Range 对象不随浏览器窗口失焦失效，扩展替换时也不需要抢焦点。

## 安全边界

- 只在**用户点击「回填」**时改 DOM，任何自动替换都不做
- 扩展权限只申请 `activeTab` / `scripting` / `nativeMessaging`，**不申请任何 host 权限**，
  也不发任何网络请求
- 宿主只认扩展 ID 白名单（manifest 的 `allowed_origins`），并校验共享 token
- `text` 长度上限 8 KB，拒绝其它方法名

## 交付

- 扩展源码放仓库 `extension/`（Chrome MV3 一份 manifest，Firefox 用 `browser_specific_settings` 兼容）
- 应用内「安装浏览器扩展」按钮：写 host manifest + 复制扩展到
  `%APPDATA%\dev.lunar.lemma\extension\` + 打开 `chrome://extensions` 并给出加载提示
- 设置项「显示回填按钮」控制 HUD 上是否出现回填入口

## 分阶段

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| 1 | `--native-host`（ping / translate）+ 终端手工喂 JSON | 命令行能看到合法响应 |
| 2 | 扩展骨架 + 普通文章替换 | 网页上选中→回填变成译文 |
| 3 | `<textarea>` / `contenteditable` / 撤销 | 三类场景都能替换且能撤回 |
| 4 | 安装按钮 + 设置开关 + Firefox 兼容 | 新机器点按钮即可装好 |
