import { useCallback, useEffect, useRef, useState } from "react";

import { listen } from "@tauri-apps/api/event";
import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t } from "../i18n";
import { ipc, isTauri } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { Accent, CaptureMode, CardSections, ModelStatus, Settings } from "../types";
import { formatHotkey } from "../utils/hotkey";

/*
  设置窗口。

  结构：左侧窄分类导航 + 右侧内容，**一次只渲染一个分类**。
  之前是一整页长列表，滚到最后要翻好几屏；分类化之后每一屏都能扫完。
  设计约束见 `docs/ui-redesign-prompts.md`：无渐变、无发光、圆角 6/8/12、只有一条陶土色指示线。
*/

type Category =
  | "trigger"
  | "capture"
  | "models"
  | "speech"
  | "ocr"
  | "compare"
  | "translate"
  | "card"
  | "ui"
  | "perf";

const NAV: { key: Category; label: string }[] = [
  { key: "trigger", label: t("settingsSectionTrigger") },
  { key: "capture", label: t("settingsSectionCapture") },
  { key: "models", label: t("settingsSectionModels") },
  { key: "speech", label: t("settingsSectionSpeech") },
  { key: "ocr", label: t("settingsSectionOcr") },
  { key: "compare", label: t("settingsSectionReader") },
  { key: "translate", label: t("settingsSectionTranslate") },
  { key: "card", label: t("settingsSectionCard") },
  { key: "ui", label: t("settingsSectionLooks") },
  { key: "perf", label: t("settingsSectionPerf") },
];

const CAPTURE_OPTIONS: { value: CaptureMode; label: string; hint: string }[] = [
  { value: "auto", label: t("settingsCaptureAuto"), hint: t("settingsCaptureAutoHint") },
  {
    value: "selectionOnly",
    label: t("settingsCaptureSelection"),
    hint: t("settingsCaptureSelectionHint"),
  },
  {
    value: "hoverOnly",
    label: t("settingsCaptureHover"),
    hint: t("settingsCaptureHoverHint"),
  },
];

const SECTION_OPTIONS: { key: keyof CardSections; label: string }[] = [
  { key: "phonetic", label: "音标" },
  { key: "translation", label: "中文释义" },
  { key: "definition", label: "英文释义" },
  { key: "tags", label: "考试标签" },
  { key: "exchange", label: "词形变化" },
];

/** 一行设置：左侧标签与说明，右侧控件。 */
function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="set-row">
      <div className="min-w-0">
        <div className="text-[14px]">{label}</div>
        {hint && (
          <p className="mt-0.5 text-[12.5px] leading-[1.55] text-[var(--hud-muted)]">{hint}</p>
        )}
      </div>
      {children && <div className="ml-auto flex shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}

function Switch({
  on,
  label,
  onChange,
}: {
  on: boolean;
  label: string;
  onChange: (next: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={() => onChange(!on)}
      className={`set-switch ${on ? "is-on" : ""}`}
    />
  );
}

/**
 * 下拉框。
 *
 * 不用原生 `<Dropdown>`：它的弹出层由系统绘制（浅色高亮、方角、字体不受控），
 * 和这套石墨界面完全不搭。这里自己画，带勾选标记与键盘焦点。
 */
function Dropdown<T extends string>({
  value,
  options,
  onChange,
  align = "right",
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (next: T) => void;
  align?: "left" | "right";
}) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement | null>(null);

  // 点外面关掉
  useEffect(() => {
    if (!open) {
      return;
    }
    const onDown = (event: PointerEvent) => {
      if (box.current && !box.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", onDown);
    return () => document.removeEventListener("pointerdown", onDown);
  }, [open]);

  const current = options.find((option) => option.value === value);

  return (
    <div className="set-dd" ref={box}>
      <button
        type="button"
        className="set-dd-btn"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((prev) => !prev)}
      >
        <span className="truncate">{current?.label ?? "—"}</span>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2 4 L5 7 L8 4" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>

      {open && (
        <div
          className={`set-dd-panel ${align === "left" ? "is-left" : ""}`}
          role="listbox"
          tabIndex={-1}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              setOpen(false);
            }
          }}
        >
          {options.map((option) => {
            const selected = option.value === value;
            return (
              <button
                key={option.value}
                type="button"
                role="option"
                aria-selected={selected}
                className={`set-dd-item ${selected ? "is-on" : ""}`}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                }}
              >
                <span className="truncate">{option.label}</span>
                {selected && (
                  <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
                    <path d="M2.5 6.5 L4.8 8.8 L9.5 3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
function Range({
  value,
  min,
  max,
  step,
  onCommit,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  onCommit: (next: number) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={draft}
      onChange={(event) => setDraft(Number(event.target.value))}
      onPointerUp={() => onCommit(draft)}
      onKeyUp={() => onCommit(draft)}
      className="set-range"
    />
  );
}

/** 文本输入：本地草稿，失焦或回车才提交，避免每敲一个字符就写盘 + 打网络。 */
function TextField({
  label,
  hint,
  value,
  onCommit,
  secret,
}: {
  label: string;
  hint?: string;
  value: string;
  onCommit: (next: string) => void;
  secret?: boolean;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);

  const commit = () => {
    if (draft !== value) {
      onCommit(draft);
    }
  };

  return (
    <label className="block border-b border-[var(--hud-border-soft)] py-2 last:border-b-0">
      <span className="block text-[14px]">{label}</span>
      <input
        type={secret ? "password" : "text"}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            commit();
          }
        }}
        className="mt-1 w-full border-b border-[var(--hud-border)] bg-transparent pb-1 font-mono text-[13.5px] text-[var(--hud-text)] outline-none placeholder:text-[var(--hud-faint)] focus:border-[var(--hud-accent)]"
        placeholder={hint}
      />
    </label>
  );
}

/** 模型目录行：状态点 + 路径 + 打开目录。 */
function ModelRow({
  label,
  ready,
  dir,
  onOpen,
  download,
}: {
  label: string;
  ready: boolean;
  dir: string;
  onOpen: () => void;
  download?: {
    /** 按钮文字，如「下载（84 MiB）」。 */
    label: string;
    /** 正在下载时的进度 0..1；null 表示没在下载。 */
    progress: number | null;
    onClick: () => void;
  };
}) {
  const busy = download?.progress != null;
  return (
    <div className="border-b border-[var(--hud-border-soft)] py-2.5 last:border-b-0">
      <div className="flex items-center gap-2">
        <span className="text-[14px]">{label}</span>
        <span className={`set-status ${ready ? "is-ok" : "is-off"}`}>
          <i />
          {ready ? t("settingsModelReady") : t("settingsModelMissing")}
        </span>
        <div className="ml-auto flex items-center gap-2">
          {download && !ready && (
            <button
              type="button"
              onClick={download.onClick}
              disabled={busy}
              className="set-btn"
            >
              {busy
                ? `${Math.round((download.progress ?? 0) * 100)}%`
                : download.label}
            </button>
          )}
          <button type="button" onClick={onOpen} className="set-btn">
            {t("settingsModelOpen")}
          </button>
        </div>
      </div>
      {busy && (
        <div className="mt-2 h-1 w-full overflow-hidden rounded-full bg-[var(--hud-border-soft)]">
          <div
            className="h-full rounded-full bg-[var(--hud-accent)] transition-[width] duration-200"
            style={{ width: `${Math.round((download?.progress ?? 0) * 100)}%` }}
          />
        </div>
      )}
      <p className="mt-1 break-all font-mono text-[12px] text-[var(--hud-faint)]">{dir}</p>
    </div>
  );
}
export function SettingsView() {
  useRuntimeInfo();
  const stats = useAppStore((state) => state.stats);
  const setStats = useAppStore((state) => state.setStats);
  const latencyMs = useAppStore((state) => state.latencyMs);
  const settings = useAppStore((state) => state.settings);
  const setSettings = useAppStore((state) => state.setSettings);

  /** 左侧导航高亮项：跟着右侧滚动位置走，不是"当前显示哪一页"。 */
  const [active, setActive] = useState<Category>("trigger");
  /** 按需下载的进度：kind → 0..1，null 表示没在下载。 */
  const [downloading, setDownloading] = useState<Record<string, number | null>>({});
  const scrollRef = useRef<HTMLElement | null>(null);
  /** 点击导航后的一小段时间内不让滚动高亮改写选中项。 */
  const lockUntil = useRef(0);

  /** 右侧滚到哪，左边就亮哪一项。 */
  const onScroll = useCallback(() => {
    const box = scrollRef.current;
    if (!box || Date.now() < lockUntil.current) {
      return;
    }
    // 滚到底部时，最后几节永远到不了顶部；此时直接判定为最后一节，
    // 否则"界面""性能基线"永远高亮不上（用户实测踩到）。
    if (box.scrollTop + box.clientHeight >= box.scrollHeight - 4) {
      setActive(NAV[NAV.length - 1]!.key);
      return;
    }
    const top = box.scrollTop + 24;
    let current: Category = NAV[0]!.key;
    for (const item of NAV) {
      const section = document.getElementById(`set-${item.key}`);
      if (section && section.offsetTop - box.offsetTop <= top) {
        current = item.key;
      }
    }
    setActive(current);
  }, []);
  const [recordingTarget, setRecordingTarget] = useState<null | "hud" | "reader" | "ocr">(null);
  const [models, setModels] = useState<ModelStatus | null>(null);
  const recording = recordingTarget !== null;
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc.perfStats().then(setStats).catch(() => undefined);
  }, [setStats]);

  /** 改一项、其余保持不变。失败时 Rust 已回滚，这里只负责显示原因。 */
  const apply = useCallback(
    (patch: Partial<Settings>) => {
      if (!settings) {
        return;
      }
      setError(null);
      const next: Settings = { ...settings, ...patch };
      // 乐观更新：界面立刻反映，等待 Rust 的结果再校正。
      setSettings(next);
      void ipc
        .updateSettings(next)
        .then(setSettings)
        .catch((reason: unknown) => {
          setError(reason instanceof Error ? reason.message : String(reason));
          void ipc.getSettings().then(setSettings).catch(() => undefined);
        });
    },
    [settings, setSettings],
  );

  // 模型状态：装没装、装在哪
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .modelStatus()
      .then(setModels)
      .catch(() => setModels(null));
  }, []);

  /** 重新读一次模型状态（下载完成后调用）。 */
  const refreshModels = useCallback(() => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .modelStatus()
      .then(setModels)
      .catch(() => setModels(null));
  }, []);

  /** 点了下载按钮：进度靠 `models://download` 事件推过来，完成后刷新状态。 */
  const startDownload = useCallback(
    (kind: "tts" | "ocr") => {
      setDownloading((prev) => ({ ...prev, [kind]: 0 }));
      void ipc
        .downloadModels(kind)
        .then((count) => {
          void ipc.logFrontend("info", `models download ${kind} ok files=${count}`);
          refreshModels();
        })
        .catch((error: unknown) => {
          void ipc.logFrontend("error", `models download ${kind} failed: ${String(error)}`);
        })
        .finally(() => {
          setDownloading((prev) => ({ ...prev, [kind]: null }));
        });
    },
    [refreshModels],
  );

  // 下载进度事件
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<{ kind: string; done: number; total: number }>("models://download", (event) => {
      const { kind, done, total } = event.payload;
      if (!total) {
        return;
      }
      setDownloading((prev) => ({ ...prev, [kind]: Math.min(1, done / total) }));
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((error: unknown) => {
        void ipc.logFrontend("error", `listen models://download failed: ${String(error)}`);
      });
    return () => unlisten?.();
  }, []);

  // Esc 关闭设置窗口（录制热键时由录制逻辑优先处理）。
  useEffect(() => {
    if (recording) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && isTauri()) {
        event.preventDefault();
        void ipc.closeSettings();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [recording]);

  // 录制期间用捕获阶段拦下按键，避免组合键被页面的其它处理器吃掉。
  useEffect(() => {
    if (!recordingTarget) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecordingTarget(null);
        return;
      }
      const combo = formatHotkey(event);
      if (!combo) {
        setError(t("settingsHotkeyRule"));
        return;
      }
      if (recordingTarget === "ocr") {
        setRecordingTarget(null);
        apply({ ocrHotkey: combo });
      } else if (recordingTarget === "reader") {
        setRecordingTarget(null);
        apply({ readerHotkey: combo });
      } else {
        setRecordingTarget(null);
        apply({ trigger: "hotkey", hotkey: combo });
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recordingTarget, apply]);

  const isDoubleControl = settings?.trigger === "doubleControl";
  const sections = settings?.cardSections;

  const recordButton = (target: "hud" | "ocr") => (
    <button
      type="button"
      onClick={() => {
        setError(null);
        setRecordingTarget(recordingTarget === target ? null : target);
      }}
      className="set-btn"
    >
      {recordingTarget === target ? t("settingsHotkeyRecording") : t("settingsHotkeyRecord")}
    </button>
  );

  const openDir = (kind: "ocr" | "tts") => {
    setError(null);
    void ipc.openModelDir(kind).catch((reason: unknown) => {
      setError(reason instanceof Error ? reason.message : String(reason));
    });
  };

  return (
    <div className="lm-panel flex flex-col text-[var(--hud-text)]">
      <style>{SET_STYLES}</style>

      <header className="flex shrink-0 items-center gap-2 border-b border-[var(--hud-border-soft)] px-4 py-2">
        <span className="text-[13.5px] text-[var(--hud-muted)]">{t("appName")}</span>
        <span className="text-[13.5px]">{t("settingsTitle")}</span>
        <button
          type="button"
          onClick={() => void ipc.closeSettings()}
          title={t("settingsClose")}
          aria-label={t("settingsClose")}
          className="set-ico ml-auto"
        >
          ✕
        </button>
      </header>

      {error && (
        <p className="shrink-0 border-b border-[var(--hud-border-soft)] px-4 py-1.5 text-[12.5px] text-[var(--hud-err)]">
          {error}
        </p>
      )}

      <div className="flex min-h-0 flex-1">
        <nav
          aria-label={t("settingsTitle")}
          className="w-[152px] shrink-0 overflow-y-auto border-r border-[var(--hud-border-soft)] py-2"
        >
          {NAV.map((item) => (
            <button
              key={item.key}
              type="button"
              onClick={() => {
                // 先定住高亮：最后两节都滚不到"顶部"，否则会被"滚到底"的判据改写成最后一节
                setActive(item.key);
                lockUntil.current = Date.now() + 700;
                document
                  .getElementById(`set-${item.key}`)
                  ?.scrollIntoView({ block: "start", behavior: "smooth" });
              }}
              className={`set-nav ${active === item.key ? "is-on" : ""}`}
            >
              {item.label}
            </button>
          ))}
        </nav>

                <main
          ref={scrollRef}
          onScroll={onScroll}
          className="min-h-0 flex-1 overflow-y-auto px-5 py-4"
        >
          <section id="set-trigger" className="mb-7">
              <h2 className="set-h">{t("settingsSectionTrigger")}</h2>
              <p className="set-desc">选词之后用什么动作把浮层叫出来。</p>
              <Row
                label={t("settingsTriggerMode")}
                hint={
                  isDoubleControl
                    ? t("settingsTriggerDoubleControlHint")
                    : t("settingsHotkeyRule")
                }
              >
                <Dropdown
                  value={isDoubleControl ? "doubleControl" : "hotkey"}
                  options={[
                    { value: "doubleControl", label: t("settingsTriggerDoubleControl") },
                    { value: "hotkey", label: t("settingsTriggerHotkey") },
                  ]}
                  onChange={(next) => apply({ trigger: next })}
                />
              </Row>
              <Row label={t("settingsTriggerCurrent")}>
                <span className="set-cap">
                  <span>{isDoubleControl ? "Ctrl" : (settings?.hotkey ?? "—")}</span>
                  {isDoubleControl && <span>Ctrl</span>}
                </span>
                {recordButton("hud")}
                {!isDoubleControl && (
                  <button
                    type="button"
                    onClick={() => apply({ trigger: "doubleControl" })}
                    className="set-btn"
                  >
                    {t("settingsTriggerResetDefault")}
                  </button>
                )}
              </Row>
          </section>

          <section id="set-capture" className="mb-7">
              <h2 className="set-h">{t("settingsSectionCapture")}</h2>
              <p className="set-desc">决定双击时优先读选区，还是读光标下的词。</p>
              <Row
                label={t("settingsCaptureMode")}
                hint={
                  CAPTURE_OPTIONS.find((item) => item.value === settings?.captureMode)?.hint
                }
              >
                <Dropdown
                  value={(settings?.captureMode ?? "auto") as CaptureMode}
                  options={CAPTURE_OPTIONS.map((item) => ({
                    value: item.value,
                    label: item.label,
                  }))}
                  onChange={(next) => apply({ captureMode: next })}
                />
              </Row>
          </section>

          <section id="set-models" className="mb-7">
              <h2 className="set-h">{t("settingsSectionModels")}</h2>
              <p className="set-desc">{t("settingsModelsHint")}</p>
              <div className="border-b border-[var(--hud-border-soft)] py-2.5">
                <div className="flex items-center gap-2">
                  <span className="text-[12.5px]">{t("settingsModelDict")}</span>
                  <span
                    className={`set-status ${models?.dict ? "is-ok" : "is-off"}`}
                  >
                    <i />
                    {models?.dict
                      ? `${t("settingsModelReady")} · ${models.dict.toLocaleString()} / ${models.zhTerms.toLocaleString()}`
                      : t("settingsModelMissing")}
                  </span>
                </div>
                <p className="mt-1 text-[11px] leading-[1.55] text-[var(--hud-muted)]">
                  {t("settingsModelDictHint")}
                </p>
              </div>
              <ModelRow
                label={t("settingsModelOcr")}
                ready={models?.ocrReady ?? false}
                dir={models?.ocrDir ?? "—"}
                onOpen={() => openDir("ocr")}
                download={{
                  label: "下载（13.6 MiB）",
                  progress: downloading.ocr ?? null,
                  onClick: () => startDownload("ocr"),
                }}
              />
              <ModelRow
                label={t("settingsModelTts")}
                ready={models?.ttsReady ?? false}
                dir={models?.ttsDir ?? "—"}
                onOpen={() => openDir("tts")}
                download={{
                  label: "下载（84 MiB）",
                  progress: downloading.tts ?? null,
                  onClick: () => startDownload("tts"),
                }}
              />
              <div className="border-b border-[var(--hud-border-soft)] py-2.5 last:border-b-0">
                <div className="flex items-center gap-2">
                  <span className="text-[12.5px]">{t("settingsModelZh")}</span>
                  <span className={`set-status ${models?.zhDefs ? "is-ok" : "is-off"}`}>
                    <i />
                    {models?.zhDefs
                      ? `${t("settingsModelReady")} · ${models.zhDefs.toLocaleString()}`
                      : t("settingsModelMissing")}
                  </span>
                </div>
                <p className="mt-1 text-[11px] leading-[1.55] text-[var(--hud-muted)]">
                  {t("settingsModelZhHint")}
                </p>
              </div>
          </section>

          <section id="set-speech" className="mb-7">
              <h2 className="set-h">{t("settingsSectionSpeech")}</h2>
              <p className="set-desc">{t("settingsSpeechSpeedHint")}</p>
              <Row label={t("settingsSpeechSpeed")}>
                <Range
                  value={settings?.speechSpeed ?? 1}
                  min={0.6}
                  max={1.6}
                  step={0.05}
                  onCommit={(value) => apply({ speechSpeed: Number(value.toFixed(2)) })}
                />
                <span className="w-11 text-right font-mono text-[13px] tabular-nums text-[var(--hud-muted)]">
                  {(settings?.speechSpeed ?? 1).toFixed(2)}×
                </span>
              </Row>
              <Row label={t("settingsSpeakOriginal")}>
                <Switch
                  on={settings?.speakOriginal ?? true}
                  label={t("settingsSpeakOriginal")}
                  onChange={(on) => apply({ speakOriginal: on })}
                />
              </Row>
              <Row label={t("settingsSpeakTranslation")}>
                <Switch
                  on={settings?.speakTranslation ?? true}
                  label={t("settingsSpeakTranslation")}
                  onChange={(on) => apply({ speakTranslation: on })}
                />
              </Row>
          </section>

          <section id="set-ocr" className="mb-7">
              <h2 className="set-h">{t("settingsSectionOcr")}</h2>
              <p className="set-desc">{t("settingsOcrToggleHint")}</p>
              <Row label={t("settingsOcrToggle")}>
                <Switch
                  on={settings?.ocrHotkeyEnabled ?? true}
                  label={t("settingsOcrToggle")}
                  onChange={(on) => apply({ ocrHotkeyEnabled: on })}
                />
              </Row>
              <Row label={t("settingsOcrHotkeyLabel")} hint={t("settingsOcrToggleOffHint")}>
                <span className="set-cap">
                  <span>{settings?.ocrHotkey ?? "—"}</span>
                </span>
                {recordButton("ocr")}
              </Row>
          </section>

          <section id="set-compare" className="mb-7">
              <h2 className="set-h">{t("settingsSectionReader")}</h2>
              <p className="set-desc">{t("settingsReaderIntro")}</p>
              <Row
                label={t("settingsCompareMode")}
                hint={
                  settings?.readerOpenMode === "off"
                    ? t("settingsReaderOffHint")
                    : t("settingsReaderHudHint")
                }
              >
                <Dropdown
                  value={settings?.readerOpenMode === "off" ? "off" : "hud"}
                  options={[
                    { value: "hud", label: t("settingsReaderHud") },
                    { value: "off", label: t("settingsReaderOff") },
                  ]}
                  onChange={(next) => apply({ readerOpenMode: next })}
                />
              </Row>
          </section>

          <section id="set-translate" className="mb-7">
              <h2 className="set-h">{t("settingsSectionTranslate")}</h2>
              <Row
                label={t("settingsZhDefs")}
                hint={
                  settings?.zhDefs === false
                    ? t("settingsZhDefsOff")
                    : t("settingsZhDefsOn")
                }
              >
                <Switch
                  on={settings?.zhDefs ?? true}
                  label={t("settingsZhDefs")}
                  onChange={(on) => apply({ zhDefs: on })}
                />
              </Row>
              <p className="set-desc">译文会落盘缓存，重启后重复段落直接命中。</p>
              <Row
                label={t("settingsTranslateService")}
                hint={
                  settings?.translateProvider === "openAi"
                    ? t("settingsOpenaiKeyHint")
                    : t("settingsTranslateGoogleHint")
                }
              >
                <Dropdown
                  value={settings?.translateProvider ?? "google"}
                  options={[
                    { value: "google", label: t("settingsTranslateGoogle") },
                    { value: "openAi", label: t("settingsTranslateOpenai") },
                  ]}
                  onChange={(next) => apply({ translateProvider: next })}
                />
              </Row>
              {settings?.translateProvider === "openAi" && (
                <div className="border-t border-[var(--hud-border-soft)] pt-1">
                  <TextField
                    label={t("settingsOpenaiBaseUrl")}
                    value={settings.openaiBaseUrl}
                    onCommit={(value) => apply({ openaiBaseUrl: value })}
                  />
                  <TextField
                    label={t("settingsOpenaiKey")}
                    value={settings.openaiApiKey}
                    secret
                    onCommit={(value) => apply({ openaiApiKey: value })}
                  />
                  <TextField
                    label={t("settingsOpenaiModel")}
                    value={settings.openaiModel}
                    onCommit={(value) => apply({ openaiModel: value })}
                  />
                </div>
              )}
              <Row label={t("settingsTargetLang")}>
                <div className="set-seg">
                  <button
                    type="button"
                    className={settings?.targetLang === "zh" ? "is-on" : ""}
                    onClick={() => apply({ targetLang: "zh" })}
                  >
                    {t("settingsTargetZh")}
                  </button>
                  <button
                    type="button"
                    className={settings?.targetLang === "en" ? "is-on" : ""}
                    onClick={() => apply({ targetLang: "en" })}
                  >
                    {t("settingsTargetEn")}
                  </button>
                </div>
              </Row>
          </section>

          <section id="set-card" className="mb-7">
              <h2 className="set-h">{t("settingsSectionCard")}</h2>
              <p className="set-desc">选择词条卡里显示哪些区块。</p>
              {SECTION_OPTIONS.map((option) => (
                <Row key={option.key} label={option.label}>
                  <Switch
                    on={sections?.[option.key] ?? true}
                    label={option.label}
                    onChange={(on) =>
                      apply({
                        cardSections: {
                          ...(sections ?? {
                            phonetic: true,
                            translation: true,
                            definition: true,
                            tags: true,
                            exchange: true,
                          }),
                          [option.key]: on,
                        },
                      })
                    }
                  />
                </Row>
              ))}
          </section>

          <section id="set-ui" className="mb-7">
              <h2 className="set-h">{t("settingsSectionLooks")}</h2>
              <p className="set-desc">尺寸、配色与提示。</p>
              <Row
                label={t("settingsSectionSize")}
                hint="内容超出这个高度后，浮层内部滚动。"
              >
                <Range
                  value={settings?.hudMaxHeight ?? 420}
                  min={220}
                  max={720}
                  step={20}
                  onCommit={(value) => apply({ hudMaxHeight: Math.round(value) })}
                />
                <span className="w-14 text-right font-mono text-[13px] tabular-nums text-[var(--hud-muted)]">
                  {settings?.hudMaxHeight ?? 420} px
                </span>
              </Row>
              <Row label={t("settingsShowHints")}>
                <Switch
                  on={settings?.showHints ?? true}
                  label={t("settingsShowHints")}
                  onChange={(on) => apply({ showHints: on })}
                />
              </Row>
              <Row label={t("settingsSectionAccent")}>
                <div className="set-seg">
                  {(
                    [
                      { value: "clay" as Accent, label: t("settingsAccentClay"), dot: "#bd7454" },
                      { value: "olive" as Accent, label: t("settingsAccentOlive"), dot: "#98a67d" },
                    ] satisfies { value: Accent; label: string; dot: string }[]
                  ).map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      className={settings?.accent === option.value ? "is-on" : ""}
                      onClick={() => apply({ accent: option.value })}
                    >
                      <span
                        className="mr-1.5 inline-block size-2 shrink-0 rounded-full"
                        style={{ background: option.dot }}
                      />
                      {option.label}
                    </button>
                  ))}
                </div>
              </Row>
          </section>

          <section id="set-perf" className="mb-7">
              <h2 className="set-h">{t("settingsSectionPerf")}</h2>
              <p className="set-desc">最近一次的实测值；不含按需加载的模型权重。</p>
              <div className="set-perf">
                <div>
                  <span>{t("settingsStartup")}</span>
                  <b>{stats ? `${stats.startup_ms.toFixed(0)} ms` : t("noData")}</b>
                </div>
                <div>
                  <span>{t("settingsLastPaint")}</span>
                  <b>
                    {latencyMs !== null
                      ? `${latencyMs.toFixed(1)} ms`
                      : stats?.last_first_paint_ms != null
                        ? `${stats.last_first_paint_ms.toFixed(1)} ms`
                        : t("noData")}
                  </b>
                </div>
                <div>
                  <span>{t("settingsPresents")}</span>
                  <b>{stats ? String(stats.hud_presents) : t("noData")}</b>
                </div>
              </div>
          </section>
        </main>
      </div>
    </div>
  );
}

/** 设置窗口自己的控件样式（与浮层同一套令牌，圆角 6/8/12）。 */
const SET_STYLES = `
  .set-nav {
    position: relative;
    display: block;
    width: 100%;
    font: inherit;
    font-size: 13.5px;
    text-align: left;
    padding: 6px 12px 6px 14px;
    border: 0;
    background: transparent;
    color: var(--hud-muted);
    cursor: pointer;
  }
  .set-nav:hover { color: var(--hud-text); }
  /* 选中态只靠左侧 2px 陶土条 + 文字变亮；不再整块铺底色（会看起来像"可以勾选的框"） */
  .set-nav.is-on { color: var(--hud-text); background: transparent; }
  .set-nav.is-on::before {
    content: "";
    position: absolute;
    left: 0;
    top: 5px;
    bottom: 5px;
    width: 2px;
    background: var(--hud-accent);
    border-radius: 0 1px 1px 0;
  }
  .set-h { margin: 0 0 2px; font-size: 14px; font-weight: 600; }
  .set-desc { margin: 0 0 12px; font-size: 13px; line-height: 1.6; color: var(--hud-muted); }
  .set-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--hud-border-soft);
  }
  .set-row:last-child { border-bottom: 0; }
  .set-switch {
    position: relative;
    width: 26px;
    height: 14px;
    border-radius: 7px;
    border: 1px solid var(--hud-border);
    background: var(--hud-raised);
    cursor: pointer;
    flex: none;
  }
  .set-switch::after {
    content: "";
    position: absolute;
    top: 1px;
    left: 1px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--hud-muted);
    transition: transform 120ms ease-out, background-color 120ms ease-out;
  }
  .set-switch.is-on { background: var(--hud-accent-soft); border-color: var(--hud-accent); }
  .set-switch.is-on::after { transform: translateX(12px); background: var(--hud-accent); }
  .set-dd { position: relative; }
  .set-dd-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 168px;
    justify-content: space-between;
    font: inherit;
    font-size: 13px;
    padding: 5px 9px 5px 11px;
    border: 1px solid var(--hud-border);
    border-radius: var(--hud-r-ctl);
    background: var(--hud-raised);
    color: var(--hud-text);
    cursor: pointer;
  }
  .set-dd-btn:hover { border-color: var(--hud-muted); }
  .set-dd-panel {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 30;
    min-width: 100%;
    padding: 4px;
    border: 1px solid var(--hud-border);
    border-radius: var(--hud-r-panel);
    background: var(--hud-raised);
    box-shadow: 0 8px 22px rgba(0, 0, 0, 0.38);
    animation: lm-fade 100ms ease-out both;
  }
  .set-dd-panel.is-left { left: 0; right: auto; }
  .set-dd-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--hud-text);
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
  }
  .set-dd-item:hover { background: var(--hud-surface); }
  .set-dd-item.is-on { color: var(--hud-accent); }
  .set-btn {
    font: inherit;
    font-size: 12.5px;
    padding: 2px 8px;
    border: 1px solid var(--hud-border);
    border-radius: var(--hud-r-ctl);
    background: transparent;
    color: var(--hud-muted);
    cursor: pointer;
  }
  .set-btn:hover { color: var(--hud-text); border-color: var(--hud-muted); }
  .set-ico {
    font: inherit;
    font-size: 12.5px;
    width: 19px;
    height: 19px;
    display: inline-grid;
    place-items: center;
    border: 1px solid transparent;
    border-radius: var(--hud-r-ctl);
    background: transparent;
    color: var(--hud-faint);
    cursor: pointer;
  }
  .set-ico:hover { color: var(--hud-text); border-color: var(--hud-border); }
  .set-cap { display: inline-flex; gap: 3px; font-family: var(--hud-mono); font-size: 12px; }
  .set-cap span {
    padding: 1px 5px;
    border: 1px solid var(--hud-border);
    border-bottom-width: 2px;
    border-radius: 4px;
    background: var(--hud-raised);
    color: var(--hud-muted);
  }
  .set-seg { display: inline-flex; border: 1px solid var(--hud-border); border-radius: var(--hud-r-ctl); overflow: hidden; }
  .set-seg button {
    font: inherit;
    font-size: 12.5px;
    padding: 3px 10px;
    border: 0;
    background: transparent;
    color: var(--hud-muted);
    cursor: pointer;
  }
  .set-seg button + button { border-left: 1px solid var(--hud-border); }
  .set-seg button.is-on { background: var(--hud-accent-soft); color: var(--hud-accent); }
  .set-status { display: inline-flex; align-items: center; gap: 5px; font-size: 12.5px; }
  .set-status i { width: 5px; height: 5px; border-radius: 50%; background: currentColor; }
  .set-status.is-ok { color: var(--hud-ok); }
  .set-status.is-off { color: var(--hud-faint); }
  .set-range {
    width: 168px;
    accent-color: var(--hud-accent);
    cursor: pointer;
  }
  .set-perf {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    border: 1px solid var(--hud-border-soft);
    border-radius: var(--hud-r-ctl);
    overflow: hidden;
  }
  .set-perf div { padding: 10px 12px; }
  .set-perf div + div { border-left: 1px solid var(--hud-border-soft); }
  .set-perf span { display: block; font-size: 12px; color: var(--hud-muted); }
  .set-perf b {
    display: block;
    margin-top: 3px;
    font-family: var(--hud-mono);
    font-size: 16px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
`;
