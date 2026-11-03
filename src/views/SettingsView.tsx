import { useCallback, useEffect, useState } from "react";

import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t } from "../i18n";
import { ipc, isTauri } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { CaptureMode, CardSections, Settings } from "../types";
import { formatHotkey } from "../utils/hotkey";

const ROADMAP = [
  { id: "M0", label: "骨架 · 单例 · 触发 · 性能基线", done: true },
  { id: "M1", label: "三档取词 + 上下文", done: true },
  { id: "M2", label: "redb 词库 + 卡片栈下探", done: true },
  { id: "M3", label: "翻译分层路由 + 双栏", done: false },
  { id: "M4", label: "PP-OCRv5 端侧 OCR", done: false },
  { id: "M5", label: "Kokoro 流式朗读", done: false },
  { id: "M6", label: "标注层 / 原地回填", done: false },
] as const;

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

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-white/10 px-3 py-2">
      <div className="text-[11px] text-[var(--hud-muted)]">{label}</div>
      <div className="mt-0.5 font-mono text-[15px]">{value}</div>
    </div>
  );
}

function Radio({
  active,
  label,
  hint,
  onClick,
}: {
  active: boolean;
  label: string;
  hint?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-start gap-2 rounded-lg border px-3 py-2 text-left transition ${
        active
          ? "border-[var(--hud-accent)] bg-white/5"
          : "border-white/10 hover:border-white/25"
      }`}
    >
      <span
        className="mt-1 size-2 shrink-0 rounded-full"
        style={{
          background: active ? "var(--hud-accent)" : "oklch(1 0 0 / 20%)",
        }}
      />
      <span className="min-w-0">
        <span className="block text-[13px]">{label}</span>
        {hint && (
          <span className="mt-0.5 block text-[11px] leading-snug text-[var(--hud-muted)]">
            {hint}
          </span>
        )}
      </span>
    </button>
  );
}

export function SettingsView() {
  useRuntimeInfo();
  const stats = useAppStore((state) => state.stats);
  const setStats = useAppStore((state) => state.setStats);
  const latencyMs = useAppStore((state) => state.latencyMs);
  const settings = useAppStore((state) => state.settings);
  const setSettings = useAppStore((state) => state.setSettings);

  const [recording, setRecording] = useState(false);
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
    if (!recording) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecording(false);
        return;
      }
      const combo = formatHotkey(event);
      if (!combo) {
        setError(t("settingsHotkeyRule"));
        return;
      }
      setRecording(false);
      apply({ trigger: "hotkey", hotkey: combo });
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [recording, apply]);

  const startup = stats ? `${stats.startup_ms.toFixed(0)} ms` : t("noData");
  const presents = stats ? String(stats.hud_presents) : t("noData");
  const lastPaint =
    latencyMs !== null
      ? `${latencyMs.toFixed(1)} ms`
      : stats?.last_first_paint_ms != null
        ? `${stats.last_first_paint_ms.toFixed(1)} ms`
        : t("noData");

  const isDoubleControl = settings?.trigger === "doubleControl";
  const sections = settings?.cardSections;

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-xl border text-[var(--hud-text)]"
      style={{ borderColor: "var(--hud-border)", background: "var(--hud-surface)" }}
    >
      <header
        data-tauri-drag-region
        className="flex shrink-0 items-center gap-2 border-b border-white/8 px-4 py-2.5"
      >
        <span className="text-[13px] font-medium">
          {t("appName")} · {t("settingsTitle")}
        </span>
        <button
          type="button"
          onClick={() => void ipc.closeSettings()}
          title={t("settingsClose")}
          aria-label={t("settingsClose")}
          className="ml-auto grid size-6 place-items-center rounded-md text-[var(--hud-muted)] transition hover:bg-white/10 hover:text-[var(--hud-text)]"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path
              d="M2 2 L10 10 M10 2 L2 10"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
            />
          </svg>
        </button>
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {/* ---- 呼出方式 ---- */}
        <section>
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionTrigger")}
          </h2>
          <div className="space-y-1.5">
            <Radio
              active={isDoubleControl}
              label={t("settingsTriggerDoubleControl")}
              hint={t("settingsTriggerDoubleControlHint")}
              onClick={() => apply({ trigger: "doubleControl" })}
            />
            <div
              className={`rounded-lg border px-3 py-2 ${
                !isDoubleControl
                  ? "border-[var(--hud-accent)] bg-white/5"
                  : "border-white/10"
              }`}
            >
              <div className="flex items-center gap-2">
                <span className="text-[13px]">{t("settingsTriggerHotkey")}</span>
                <span className="ml-auto font-mono text-[12px] text-[var(--hud-accent)]">
                  {settings?.hotkey ?? "—"}
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setError(null);
                    setRecording((value) => !value);
                  }}
                  className="rounded-md border border-white/15 px-2 py-0.5 text-[11px] hover:border-white/35"
                >
                  {recording ? t("settingsHotkeyRecording") : t("settingsHotkeyRecord")}
                </button>
              </div>
              <div className="mt-1 text-[11px] text-[var(--hud-muted)]">
                {t("settingsHotkeyRule")}
              </div>
            </div>
          </div>
        </section>

        {/* ---- 取词模式 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionCapture")}
          </h2>
          <div className="space-y-1.5">
            {CAPTURE_OPTIONS.map((option) => (
              <Radio
                key={option.value}
                active={settings?.captureMode === option.value}
                label={option.label}
                hint={option.hint}
                onClick={() => apply({ captureMode: option.value })}
              />
            ))}
          </div>
        </section>

        {/* ---- 卡片内容 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionCard")}
          </h2>
          <div className="flex flex-wrap gap-1.5">
            {SECTION_OPTIONS.map((option) => {
              const active = sections?.[option.key] ?? false;
              return (
                <button
                  key={option.key}
                  type="button"
                  onClick={() =>
                    sections &&
                    apply({
                      cardSections: { ...sections, [option.key]: !active },
                    })
                  }
                  className={`rounded-md border px-2 py-1 text-[12px] transition ${
                    active
                      ? "border-[var(--hud-accent)] text-[var(--hud-accent)]"
                      : "border-white/12 text-[var(--hud-muted)] hover:border-white/30"
                  }`}
                >
                  {option.label}
                </button>
              );
            })}
          </div>
        </section>

        {/* ---- HUD 最大高度 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 flex items-center gap-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionSize")}
            <span className="ml-auto font-mono text-[12px] text-[var(--hud-accent)]">
              {settings?.hudMaxHeight ?? 480} px
            </span>
          </h2>
          <input
            type="range"
            min={160}
            max={720}
            step={20}
            value={settings?.hudMaxHeight ?? 480}
            onChange={(event) =>
              apply({ hudMaxHeight: Number(event.target.value) })
            }
            className="w-full accent-[var(--hud-accent)]"
          />
        </section>

        {error && (
          <p className="mt-2 rounded-md border border-red-400/30 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
            {t("settingsHotkeySaveFailed")}：{error}
          </p>
        )}

        {/* ---- 界面 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionLooks")}
          </h2>
          <button
            type="button"
            onClick={() => apply({ showHints: !(settings?.showHints ?? true) })}
            className={`flex w-full items-start gap-2 rounded-lg border px-3 py-2 text-left transition ${
              settings?.showHints ?? true
                ? "border-[var(--hud-accent)] bg-white/5"
                : "border-white/10 hover:border-white/25"
            }`}
          >
            <span
              className="mt-1 grid size-3 shrink-0 place-items-center rounded-[3px] border"
              style={{
                borderColor: (settings?.showHints ?? true)
                  ? "var(--hud-accent)"
                  : "oklch(1 0 0 / 25%)",
              }}
            >
              {(settings?.showHints ?? true) && (
                <span
                  className="block size-1.5 rounded-[1px]"
                  style={{ background: "var(--hud-accent)" }}
                />
              )}
            </span>
            <span className="min-w-0">
              <span className="block text-[13px]">{t("settingsShowHints")}</span>
              <span className="mt-0.5 block text-[11px] leading-snug text-[var(--hud-muted)]">
                {t("settingsShowHintsHint")}
              </span>
            </span>
          </button>
        </section>

        {/* ---- 性能 ---- */}
        <section className="mt-5">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionPerf")}
          </h2>
          <div className="grid grid-cols-3 gap-2">
            <Stat label={t("settingsStartup")} value={startup} />
            <Stat label={t("settingsPresents")} value={presents} />
            <Stat label={t("settingsLastPaint")} value={lastPaint} />
          </div>
        </section>

        {/* ---- 里程碑 ---- */}
        <section className="mt-5 pb-2">
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionRoadmap")}
          </h2>
          <ul className="space-y-1">
            {ROADMAP.map((item) => (
              <li
                key={item.id}
                className="flex items-center gap-2 rounded-md px-2 py-1 text-[12px]"
              >
                <span
                  className="font-mono text-[11px]"
                  style={{
                    color: item.done ? "var(--hud-accent)" : "var(--hud-muted)",
                  }}
                >
                  {item.id}
                </span>
                <span
                  className={
                    item.done ? "text-[var(--hud-text)]" : "text-[var(--hud-muted)]"
                  }
                >
                  {item.label}
                </span>
                {item.done && (
                  <span className="ml-auto text-[11px] text-[var(--hud-accent)]">完成</span>
                )}
              </li>
            ))}
          </ul>
        </section>
      </main>
    </div>
  );
}
