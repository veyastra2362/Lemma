import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { PaintSample, RuntimeStats } from "../types";

/** 是否跑在 Tauri 宿主里（`pnpm dev` 直接开浏览器时为 false）。 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 命令名是 Rust 侧的契约，改名等于破坏前端。
 * 这里集中收口，避免各处散落字符串。
 */
export const ipc = {
  hudHide: () => invoke<void>("hud_hide"),
  hudToggle: () => invoke<void>("hud_toggle"),
  openSettings: () => invoke<void>("open_settings"),
  perfStats: () => invoke<RuntimeStats>("perf_stats"),
  reportFirstPaint: (seq: number) => invoke<void>("report_first_paint", { seq }),
};

/** 监听 HUD 呼出事件，拿到本次序号。 */
export function onHudPresented(
  handler: (seq: number) => void,
): Promise<UnlistenFn> {
  return listen<number>("hud://presented", (event) => handler(event.payload));
}

/** 监听首帧结算结果，用于在界面上展示真实延迟。 */
export function onFirstPaint(
  handler: (sample: PaintSample) => void,
): Promise<UnlistenFn> {
  return listen<PaintSample>("perf://first-paint", (event) =>
    handler(event.payload),
  );
}
