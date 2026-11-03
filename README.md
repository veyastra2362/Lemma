# Trans

高频深度阅读场景下的轻量桌面辅助系统：**全局划词 → 无限下探查词 → 端侧 OCR / 翻译 → 原地回填**。

> 当前状态：**M0（骨架）已落地**。单例常驻 + 托盘 + 全局热键 + HUD 预热 + 呼出延迟基线 + CI 门禁。
> 详细路线图见 [`docs/PLAN.md`](docs/PLAN.md)。

---

## 技术栈

| 层 | 选型 |
| --- | --- |
| 桌面壳 | Tauri v2（单实例 / 托盘 / 全局热键 / 多窗口预热） |
| 前端 | React 19 · TypeScript · Vite · Tailwind v4 · zustand |
| 核心 | Rust 2024，`core/`（跨平台逻辑）+ `platform/`（OS 契约与后端） |
| 端侧 AI | ONNX Runtime（PP-OCRv5 / Kokoro）— M4 / M5 |
| 词库 | redb + 内存索引 — M2 |

---

## 开发环境

- Rust 1.85+（edition 2024），Windows 需 MSVC 工具链
- Node.js 20+ / pnpm 11+
- Windows 10 2004+ / 11（自带 WebView2）

```powershell
pnpm install
pnpm tauri dev      # 启动后按 Alt+Q 呼出 HUD；托盘图标右键有菜单
```

### 质量门禁

```powershell
pnpm typecheck                              # tsc --noEmit
pnpm test                                   # vitest
cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings   # 零 warning
cargo test
```

---

## 架构纪律

```
src-tauri/src/
├── lib.rs          组合根
├── commands.rs     IPC 适配层（只转换与转发，命令名是前端契约）
├── core/           跨平台业务逻辑，禁止 cfg(target_os) 与原生 API
└── platform/       OS 契约与后端
    ├── placement.rs  纯几何（可在任意平台跑单测）
    ├── window.rs     WindowOps 契约
    └── windows/      Win32 / UIA 实现
```

- `core/` 只能通过 `crate::platform` 导出的函数与类型触达操作系统；
- `cfg(target_os)` 只允许出现在 `platform/mod.rs` 与各后端内部；
- 新增一项 OS 能力 = 加一个契约函数，所有后端都要实现。

这三条会在 CI 里由 `scripts/check-architecture.ps1` 守住。

---

## 安全边界（从第一行代码就定死）

- **不装全局键盘钩子**（无 `SetWindowsHookEx(WH_KEYBOARD_LL)`），只用 `RegisterHotKey`；
- **不注入 DLL、不打补丁、不 hook 窗口消息**；
- 模拟输入仅用于「回填」这一条兜底路径，且严格保存/恢复剪贴板全格式。

这样能避开绝大多数杀软 / EDR 误报，也避开模拟输入的合规灰区。

---

## 性能口径（写清楚，免得被追问）

性能数字必须带口径，否则没有意义。完整测量方法与明细见 [`docs/PERF.md`](docs/PERF.md)。

### M0 实测（Windows 11 / Rust 1.99.1 / tauri 2.11.6）

| 指标 | 实测 |
| --- | --- |
| 冷启动到托盘就绪 | 550 – 575 ms |
| **Rust 主进程**私有内存 | **6.3 MB**（工作集 27 – 29 MB） |
| 整个进程树私有内存 | **166 MB**（7 个进程，其中 6 个是 WebView2） |
| HUD 首帧（首次呼出） | 22.3 ms |
| HUD 首帧（热态） | 4.8 – 5.8 ms |

**「常驻 30–60 MB」只能指 Rust 主进程**。整个进程树私有内存约 166 MB，
96% 是 WebView2 的浏览器/渲染/GPU/网络进程——这是 Tauri（以及任何 WebView 方案）的底噪。
对外报数字必须写清是哪一个，后续所有里程碑的模型权重也是内存大头，
所以计划里坚持「模型不常驻 + 空闲卸载」。

参考实现 `LunarFlow42/TransPortal` 的实测数据同样值得对照：900×180 中英混排 OCR
首次 **805 ms**、热态中位 **272 ms**，无界面探针峰值工作集 **79.4 MiB**。
即最初设想的「50 ms / 45 MB」并未达成。本项目刻意不沿用那些数字。
