export interface RuntimeStats {
  /** 进程启动到 setup 完成（毫秒）。 */
  startup_ms: number;
  /** 累计呼出 HUD 次数。 */
  hud_presents: number;
  /** 最近一次「热键 → 首帧」耗时（毫秒）。 */
  last_first_paint_ms: number | null;
}

export interface PaintSample {
  seq: number;
  total_ms: number;
}

/** 呼出方式，与 Rust 侧 `core::settings::Trigger` 一一对应。 */
export type Trigger = "doubleControl" | "hotkey";

export interface Settings {
  trigger: Trigger;
  /** 形如 `Alt+Q`，仅在 trigger === "hotkey" 时生效。 */
  hotkey: string;
}
