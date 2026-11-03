import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Capture,
  PaintSample,
  RuntimeStats,
  Settings,
  Trigger,
} from "../types";

/** 是否跑在 Tauri 宿主里（`pnpm dev` 直接开浏览器时为 false）。 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 带重试的 IPC 调用。
 *
 * WebView2 的 IPC 通道建立和页面模块执行之间存在竞争：窗口刚起来时
 * `__TAURI_INTERNALS__` 已经存在，但 `invoke` 可能第一次就被拒。
 * 这类失败必须重试，不能当成业务错误。
 */
async function withRetry<T>(
  label: string,
  op: () => Promise<T>,
  attempts = 8,
  delayMs = 60,
): Promise<T> {
  let lastError: unknown;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      return await op();
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
  }
  throw new Error(
    `${label} 失败：${lastError instanceof Error ? lastError.message : String(lastError)}`,
  );
}

/**
 * 命令名是 Rust 侧的契约，改名等于破坏前端。
 * 这里集中收口，避免各处散落字符串。
 */
export const ipc = {
  hudHide: () => invoke<void>("hud_hide"),
  hudToggle: () => invoke<void>("hud_toggle"),
  openSettings: () => invoke<void>("open_settings"),
  frontendReady: (label: string) =>
    withRetry("frontend_ready", () =>
      invoke<void>("frontend_ready", { label }),
    ),
  perfStats: () => invoke<RuntimeStats>("perf_stats"),
  lastCapture: () =>
    withRetry("last_capture", () => invoke<Capture | null>("last_capture")),
  getSettings: () =>
    withRetry("get_settings", () => invoke<Settings>("get_settings")),
  /** 应用失败（如热键被占用）会 reject，Rust 侧已回滚到旧配置。 */
  updateSettings: (next: { trigger: Trigger; hotkey: string }) =>
    invoke<Settings>("update_settings", next),
  reportFirstPaint: (seq: number) => invoke<void>("report_first_paint", { seq }),
};

/** 监听 HUD 呼出事件，拿到本次序号。 */
export function onHudPresented(
  handler: (seq: number) => void,
): Promise<UnlistenFn> {
  return withRetry("listen(hud://presented)", () =>
    listen<number>("hud://presented", (event) => handler(event.payload)),
  );
}

/** 监听首帧结算结果，用于在界面上展示真实延迟。 */
export function onFirstPaint(
  handler: (sample: PaintSample) => void,
): Promise<UnlistenFn> {
  return withRetry("listen(perf://first-paint)", () =>
    listen<PaintSample>("perf://first-paint", (event) => handler(event.payload)),
  );
}
