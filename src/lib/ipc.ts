import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  Capture,
  Entry,
  PaintSample,
  ReaderContent,
  RuntimeStats,
  ModelStatus,
  Settings,
  SpeakResult,
  TranslateResult,
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
  closeSettings: () => invoke<void>("close_settings"),
  frontendReady: (label: string) =>
    withRetry("frontend_ready", () =>
      invoke<void>("frontend_ready", { label }),
    ),
  perfStats: () => invoke<RuntimeStats>("perf_stats"),
  /** 把前端日志写进 Rust 的日志文件（GUI 的 console 我看不到）。 */
  logFrontend: (level: "error" | "warn" | "info", message: string) =>
    invoke<void>("log_frontend", { level, message }),
  lastCapture: () =>
    withRetry("last_capture", () => invoke<Capture | null>("last_capture")),
  getSettings: () =>
    withRetry("get_settings", () => invoke<Settings>("get_settings")),
  /** 传完整设置对象。应用失败（如热键被占用）会 reject，Rust 侧已回滚到旧配置。 */
  updateSettings: (next: Settings) => invoke<Settings>("update_settings", { next }),
  /** 按内容高度调整 HUD（逻辑像素），Rust 侧会钳到设置里的上限。 */
  resizeHud: (height: number) => invoke<void>("resize_hud", { height }),
  /** Deck 下探查词。未收录返回 null。 */
  lookupWord: (word: string) => invoke<Entry | null>("lookup_word", { word }),
  /** 本地模型（OCR / TTS）的安装状态与目录。 */
  modelStatus: () =>
    withRetry("model_status", () => invoke<ModelStatus>("model_status")),
  /** 按需下载可选模型，返回下载的文件数。 */
  downloadModels: (kind: "tts") => invoke<number>("download_models", { kind }),
  openModelDir: (kind: "ocr" | "tts") =>
    invoke<void>("open_model_dir", { kind }),
  /** 只合成不播放（流式朗读用）。 */
  synthesizeSpeech: (text: string) =>
    invoke<SpeakResult>("synthesize_speech", { text }),
  /** 播放一个已合成好的 WAV。 */
  playAudio: (path: string) => invoke<void>("play_audio", { path }),
  /** 朗读一段文本（合成 + 播放，一次性）。 */
  speakText: (text: string) => invoke<SpeakResult>("speak_text", { text }),
  stopSpeaking: () => invoke<void>("stop_speaking"),
  speechReady: () => withRetry("speech_ready", () => invoke<boolean>("speech_ready")),
  /** 框选区域（物理像素）→ 截屏 → OCR → 弹 HUD。 */
  runRegionOcr: (rect: { x: number; y: number; width: number; height: number }) =>
    invoke<Capture>("run_region_ocr", rect),
  startRegionSelect: () => invoke<void>("start_region_select"),
  cancelRegionSelect: () => invoke<void>("cancel_region_select"),
  /**
   * 本框选窗口左上角的**物理**坐标（多屏可能为负）。
   *
   * 每个显示器一个框选窗口，所以这个窗口的 DPI 恒等于它所在显示器的缩放，
   * "物理位置 + CSS × devicePixelRatio" 就一定能算出正确的物理矩形。
   */
  regionWindowOrigin: async (): Promise<{ x: number; y: number }> => {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const position = await getCurrentWindow().innerPosition();
    return { x: position.x, y: position.y };
  },

  /** 打开双栏窗口并显示这段文本。 */
  openReader: (text: string, source: string) =>
    invoke<void>("open_reader", { text, source }),
  /** 双栏窗口拉取内容。 */
  readerContent: () =>
    withRetry("reader_content", () => invoke<ReaderContent | null>("reader_content")),
  closeReader: () => invoke<void>("close_reader"),
  /** 翻译一段文本（走 Rust 的分级路由 + 缓存）。失败会 reject。 */
  translateText: (text: string) =>
    invoke<TranslateResult>("translate_text", { text }),
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

/** 双栏窗口每次被打开时，Rust 会广播这个事件要求重新拉内容。 */
export function onReaderReload(handler: () => void): Promise<UnlistenFn> {
  return listen("reader://reload", () => handler());
}

/** 仅用于诊断：Rust 侧发起的 IPC 冒烟（`LEMMA_READER_SMOKE_IPC`）。 */
export function onReaderSmoke(
  handler: (text: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("reader://smoke", (event) => handler(event.payload));
}

/** 监听设置变更（设置窗口改了设置后，HUD 靠这个同步）。 */
export function onSettingsChanged(
  handler: (settings: Settings) => void,
): Promise<UnlistenFn> {
  return withRetry("listen(settings://changed)", () =>
    listen<Settings>("settings://changed", (event) => handler(event.payload)),
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
