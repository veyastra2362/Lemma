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

/** 翻译服务。 */
export type TranslateProvider = "google" | "openAi";

/** 目标语言。 */
export type TranslateLang = "zh" | "en";

/** 强调色。 */
export type Accent = "clay" | "olive";

/** 双栏窗口的打开方式。 */

/** 双栏窗口的内容（块已由 Rust 侧切好）。 */
export interface ModelStatus {
  /** 英汉词库条数（ECDICT）。0 = 没装。 */
  dict: number;
  /** 中文词条数（CC-CEDICT）。 */
  zhTerms: number;
  /** 汉语词典条数（0 = 没导入）。 */
  zhDefs: number;
  ocrReady: boolean;
  ocrDir: string;
  ttsReady: boolean;
  ttsDir: string;
}

export interface SpeakResult {
  durationSecs: number;
  synthesisMs: number;
  /** 合成出来的 WAV 路径（流式朗读自己排播放用）。 */
  path: string;
}

export interface TranslateResult {
  text: string;
  provider: string;
  elapsedMs: number;
  cached: boolean;
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
  /** 翻译走哪个服务。 */
  speechSpeed: number;
  /** 是否启用「中文释义」（点释义里的中文词查汉语词典）。 */
  zhDefs: boolean;
  /** 是否允许浏览器扩展用本应用回填。 */
  extensionWriteBack: boolean;
  /** HUD 上是否显示「回填」按钮。 */
  hudWriteBack: boolean;
  /** 光标离开浮层一定距离就自动收起（开着时隐藏 ✕）。 */
  autoHideByCursor: boolean;
  /** 强调色（整套 UI 只有一个）。 */
  accent: Accent;
  /** 卡片上是否显示「朗读原文」。 */
  speakOriginal: boolean;
  /** 卡片上是否显示「朗读译文」。 */
  speakTranslation: boolean;
  ocrHotkeyEnabled: boolean;
  ocrHotkey: string;
  translateProvider: TranslateProvider;
  targetLang: TranslateLang;
  openaiBaseUrl: string;
  openaiApiKey: string;
  openaiModel: string;
}

/** 取词命中的档位，与 Rust 侧 `core::selection::Tier` 对应。 */
export type CaptureTier = "selection" | "word" | "clipboard" | "ocr";

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
  /** 这段文本该走翻译而不是查词（规则在 Rust 侧 `translate::is_single_word`）。 */
  needsTranslation: boolean;
  /** 中文多词切分出的候选词；点一个就下探。 */
  candidates: string[];
  /** 来源进程名，诊断用。 */
  app: string;
  elapsedMs: number;
  error: string | null;
}
/** 浏览器扩展的安装状态。 */
export interface ExtensionStatus {
  installed: boolean;
  dir: string;
  extensionId: string;
  /** 扩展的 GitHub 下载地址。 */
  downloadUrl: string;
}
