# 参考实现调研：LunarFlow42/TransPortal

> 结论先说：**这是一个和我们目标几乎完全重合的成熟实现**（Rust ~17.8k 行 + TS ~13.3k 行）。
> 它**没有 LICENSE 文件**（README 声称 MIT，但仓库里没有），因此我们**不拷贝它的源码**，
> 只把它当成"架构评审 + 踩坑清单"来读。本文记录哪些结论可以直接吸收、哪些必须自己重新验证。

调研时间：2026-11-03，master 分支，克隆到 `D:\Code\_ref\TransPortal`（不在本仓库内）。

---

## 1. 可以直接吸收的结论（省掉我们踩坑）

### 1.1 窗口与 WebView2

| 结论 | 出处 | 我们怎么用 |
| --- | --- | --- |
| 透明窗口必须加 `"noRedirectionBitmap": true` | `tauri.conf.json` | 已用于 `shell` / `settings` / `ocr-region` |
| 显示设置窗口要两步：先 `window.show()`，再 `webview.set_size()` + `webview.show()` | `lib.rs::show_settings_window` | 已实现在 `core::shell::present_settings` |
| Win32 `SW_HIDE` 之后必须 `window.show()` + `SW_SHOW` + `set_focus()` 一起做，否则出现"宿主显示了但 WebView 空白" | `lib.rs` 注释 | 已固化为 `WindowOps::force_show/force_hide` + `AGENTS.md` 不变式 |
| `single-instance` 必须是**第一个**注册的插件 | `lib.rs` 注释 | 已遵守 |
| 窗口单例预热，呼出路径上绝不重建窗口 | `README.md` | 已遵守 |

### 1.2 架构分层的具体切法

- `core/` 与 `platform/` 的边界用**契约 trait** 划：`WindowOps` / `SelectionOps` / `ClipboardOps` /
  `GestureSource` / `DesktopCapture` / `SystemOps`，`Backend` trait 把它们聚成一个关联类型集合。
  → 我们的 `platform/mod.rs` 从最简单的两个函数（`cursor_position` / `work_area_at`）起步，
  M1 取词时再按这个模式长出 `SelectionOps`。
- `cfg(target_os)` 只出现在 `platform/mod.rs` 与后端内部，并用脚本在 CI 里强制。
  → 已实现 `scripts/check-architecture.ps1`。
- 纯逻辑（`placement.rs` / `imaging.rs` / `key_sequence.rs`）不碰 OS API，因此在任何平台都能跑单测。
  → 我们的 `placement.rs` 就是照这个思路写的，7 个单测覆盖边界翻转与负坐标副屏。

### 1.3 功能设计上值得抄的"点子"（不抄代码）

| 点子 | 说明 |
| --- | --- |
| 取词三档降级 | UIA `TextPattern` 优先 → 剪贴板兜底；失败时明确降级而不是报错 |
| 多通道截图探针 | DXGI / WGC-Window / WGC-Monitor / GDI 按前台窗口属性自动调度，并带诊断报告 |
| 双按 C 触发 | 0.5s 内连按两次 C 才翻译，普通复制不被抢占 |
| 单键修饰"落墨手势" | 按住单键拖选，松键即触发，省掉一次按键 |
| 截图翻译的三种呈现 | 双栏对照 / 原位覆盖 / 纯提取入剪贴板 |
| 自动区域翻译 | 框选固定区域后持续监测像素变化并更新译文 |
| 内置框选器 | 按下热键先**冻结整个虚拟桌面**再选区，不调用系统截图工具、不污染剪贴板 |
| 词库源可插拔 | `DictSource` trait + `LookupService` 编排，前端可启用/排序 |
| 记忆修剪两阶段 | `K32EmptyWorkingSet` + `SetProcessWorkingSetSize` |

### 1.4 工程纪律

- `AGENTS.md` 里那套「Level 1 秒级单测 / Level 2 commit 前质量闭环」的分级验证流很务实：
  按模块过滤跑单测（`cargo test platform::placement::tests` 0.7s），避免每次都链接大依赖。
- 把「多窗口声明 / Win32 与 Tao 状态同步 / IPC 同步性」写成**不变式**并用测试守住
  （`systemInvariants.test.ts`）。我们已把这几条抄进 `AGENTS.md`。

---

## 2. 必须自己重新验证的地方

### 2.1 性能数字与原始设想差距很大

`docs/OCR.md` 原文（作者自己的诚实记录）：

> 当前样例未达到 `<50 ms` / `<50 MB` 的原先性能设想；首轮包含资源校验与模型加载，
> 后续轮次复用会话，均不包括框选、剪贴板和翻译请求。

实测（900×180 中英混排，ONNX PP-OCRv5 FP32，CPU）：

| 指标 | 实测 |
| --- | --- |
| 首次识别 | **805 ms** |
| 热态识别（19 次中位数） | **272 ms**（区间 253–645 ms） |
| 无界面探针进程峰值工作集 | **79.4 MiB** |
| 私有内存峰值 | **67.1 MiB** |
| NSIS 安装包 | ~20.5 MB |
| 安装后展开（不含 WebView2 / 词库 / 用户数据） | ~35.4 MB |
| OCR 三份模型 + 运行库核心 | ~27.6 MiB |

**对我们的影响**：宣传口径必须改成
「OCR 百毫秒级（冷启动含模型加载约 0.8s），常驻内存 30–60MB 指模型未加载时的稳态」。
`README.md` 已经按这个口径写，后续任何 benchmark 都必须区分冷启动 / 热态、含不含模型。

### 2.2 依赖选型上可以直接省掉的探索

作者已经跑通的组合（我们据此定版本，不必重新试错）：

- `ort = "=2.0.0-rc.15"` + `features = ["load-dynamic", "api-22"]`，配 ONNX Runtime **1.22.0** 动态库
  —— `load-dynamic` 是关键：不把 ORT 静态编进二进制，安装包才压得住。
- `misaki-rs` 提供 Kokoro 需要的音素化，省掉自己移植 misaki。
- 模型来自 RapidOCR 发布的 PP-OCRv5 mobile det/rec + `ch_PP-LCNet_x0_25_textline_ori_cls_mobile`，
  字典从模型元数据里读。
- 词库用 ECCEDICT CSV → redb；`ecdict.mini.csv` 只是 53 词样例，不能当真词库。
- Windows 自带 `Windows.Media.Ocr` 可作为 ONNX 之外的第二引擎（零下载、零内存，但精度/语言依赖系统包）。

> 这些版本组合我**没有**直接写进 `Cargo.toml`——M0 不需要它们。等 M4/M5 落地时按需引入，
> 并自己在目标机器上验证，不默认"他用得起来我这里也用得起来"。

---

## 3. 它没解决、我们要做得更好的地方

1. **Deck 只是个组件，不是数据模型。** `LookupDeck.tsx` 是渲染层，缺少"会话树 + 分支回看 + 回路检测"。
   我们的方案：`nodes: HashMap<CardId, Card>` + `active_path: Vec<CardId>`，回退不重查。
2. **翻译与词库耦合得较松但没有统一成本控制。** 缺语义缓存与请求合并，深度阅读场景重复段落很多。
3. **没有性能门禁。** 仓库里有 smoke 脚本，但没有"低于 SLO 就红 CI"的机制。
   我们的 `scripts/perf-baseline.ps1` 至少把冷启动与 RSS 沉淀成 JSON 便于对比。
4. **测试覆盖偏交互，缺 golden fixture。** OCR 只有少量固定图断言行数，没有逐字段对拍 Python 版。
5. **`docs/OCR.md` 自己承认**部分功能（自动区域翻译）代码写完但**未完成桌面端验收**。
   这正是我们要避免的：里程碑验收标准写死、不通过就不算完成。

---

## 4. 明确不做的事

- 不拷贝、不翻译、不"参考改写"它的源码文件。
- 不引用它的图标、截图、字体等资源。
- 不沿用它的性能宣传数字（见 §2.1）。
- 如果将来确实想复用其中某段实现，必须先解决授权（联系作者取得 MIT 授权或明确书面许可）。
