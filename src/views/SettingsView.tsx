import { useCallback, useEffect, useState } from "react";

import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t } from "../i18n";
import { ipc, isTauri } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { Trigger } from "../types";
import { formatHotkey } from "../utils/hotkey";

const ROADMAP = [
  { id: "M0", label: "骨架 · 单例 · 触发 · 性能基线", done: true },
  { id: "M1", label: "UIA 三档取词 + 上下文", done: false },
  { id: "M2", label: "redb 百万词库 + Deck 下探", done: false },
  { id: "M3", label: "翻译分层路由 + 双栏", done: false },
  { id: "M4", label: "PP-OCRv5 端侧 OCR", done: false },
  { id: "M5", label: "Kokoro 流式朗读", done: false },
  { id: "M6", label: "标注层 / 原地回填", done: false },
] as const;

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-white/10 px-3 py-2">
      <div className="text-[11px] text-[var(--hud-muted)]">{label}</div>
      <div className="mt-0.5 font-mono text-[15px]">{value}</div>
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

  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc.perfStats().then(setStats).catch(() => undefined);
  }, [setStats]);

  const apply = useCallback(
    (trigger: Trigger, hotkey: string) => {
      setError(null);
      void ipc
        .updateSettings({ trigger, hotkey })
        .then(setSettings)
        .catch((reason: unknown) => {
          setError(reason instanceof Error ? reason.message : String(reason));
        });
    },
    [setSettings],
  );

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
      apply("hotkey", combo);
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
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <section>
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionTrigger")}
          </h2>

          <div className="space-y-1.5">
            <button
              type="button"
              onClick={() => apply("doubleControl", settings?.hotkey ?? "Alt+Q")}
              className={`flex w-full items-start gap-2 rounded-lg border px-3 py-2 text-left transition ${
                isDoubleControl
                  ? "border-[var(--hud-accent)] bg-white/5"
                  : "border-white/10 hover:border-white/25"
              }`}
            >
              <span
                className="mt-1 size-2 shrink-0 rounded-full"
                style={{
                  background: isDoubleControl
                    ? "var(--hud-accent)"
                    : "oklch(1 0 0 / 20%)",
                }}
              />
              <span>
                <span className="block text-[13px]">
                  {t("settingsTriggerDoubleControl")}
                </span>
                <span className="mt-0.5 block text-[11px] text-[var(--hud-muted)]">
                  {t("settingsTriggerDoubleControlHint")}
                </span>
              </span>
            </button>

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

          {error && (
            <p className="mt-2 rounded-md border border-red-400/30 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
              {t("settingsHotkeySaveFailed")}：{error}
            </p>
          )}
        </section>

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

        <section className="mt-5">
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
                  <span className="ml-auto text-[11px] text-[var(--hud-accent)]">
                    完成
                  </span>
                )}
              </li>
            ))}
          </ul>
        </section>
      </main>
    </div>
  );
}
