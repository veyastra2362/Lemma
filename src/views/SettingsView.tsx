import { useEffect } from "react";

import { ipc, isTauri } from "../lib/ipc";
import { t } from "../i18n";
import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { useAppStore } from "../store/useAppStore";

const ROADMAP = [
  { id: "M0", label: "骨架 · 单例 · 热键 · 性能基线", done: true },
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
  const hotkey = useAppStore((state) => state.hotkey);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc.perfStats().then(setStats);
  }, [setStats]);

  const startup = stats ? `${stats.startup_ms.toFixed(0)} ms` : t("becauseNoData");
  const presents = stats ? String(stats.hud_presents) : t("becauseNoData");
  const lastPaint =
    latencyMs !== null
      ? `${latencyMs.toFixed(1)} ms`
      : stats?.last_first_paint_ms != null
        ? `${stats.last_first_paint_ms.toFixed(1)} ms`
        : t("becauseNoData");

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
          className="ml-auto rounded-md border border-white/10 px-2 py-0.5 text-[11px] text-[var(--hud-muted)] hover:border-white/25 hover:text-[var(--hud-text)]"
          onClick={() => void ipc.hudHide()}
        >
          {t("hudHintEscape")}
        </button>
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <section>
          <h2 className="mb-2 text-[12px] font-medium tracking-wide text-[var(--hud-muted)]">
            {t("settingsSectionPerf")}
          </h2>
          <div className="grid grid-cols-3 gap-2">
            <Stat label={t("settingsStartup")} value={startup} />
            <Stat label={t("settingsPresents")} value={presents} />
            <Stat label={t("settingsLastPaint")} value={lastPaint} />
          </div>
          <div className="mt-2">
            <Stat
              label={t("settingsHotkey")}
              value={hotkey ?? t("hudHintHotkeyUnavailable")}
            />
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
