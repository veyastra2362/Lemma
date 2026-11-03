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

/** 取词模式，与 Rust 侧 `core::settings::CaptureMode` 一一对应。 */
export type CaptureMode = "auto" | "selectionOnly" | "hoverOnly";

/** 卡片上显示哪些区块。 */
export interface CardSections {
  phonetic: boolean;
  translation: boolean;
  definition: boolean;
  tags: boolean;
  exchange: boolean;
}

export interface Settings {
  trigger: Trigger;
  /** 形如 `Alt+Q`，仅在 trigger === "hotkey" 时生效。 */
  hotkey: string;
  captureMode: CaptureMode;
  /** HUD 自适应的最大高度（逻辑像素）。 */
  hudMaxHeight: number;
  /** 是否显示 HUD 底部那排快捷键提示。 */
  showHints: boolean;
  cardSections: CardSections;
}

/** 取词命中的档位，与 Rust 侧 `core::selection::Tier` 对应。 */
export type CaptureTier = "selection" | "word" | "clipboard";

/** 词库词条，与 Rust 侧 `core::dict::Entry` 对应。 */
export interface Entry {
  word: string;
  phonetic: string;
  /** 中文释义，多义项用 \n 分隔。 */
  translation: string;
  definition: string;
  pos: string;
  tag: string;
  exchange: string;
  bnc: number;
  frq: number;
}

export interface Capture {
  text: string;
  sentence: string;
  paragraph: string;
  /** null 表示这次没取到。 */
  tier: CaptureTier | null;
  /** 词库命中的词条；未收录或没装词库时为 null。 */
  entry: Entry | null;
  /** 来源进程名，诊断用。 */
  app: string;
  elapsedMs: number;
  error: string | null;
}
