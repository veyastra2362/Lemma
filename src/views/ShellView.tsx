import { useHudLifecycle } from "../hooks/useHudLifecycle";
import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t } from "../i18n";
import { useAppStore } from "../store/useAppStore";

function Hint({ label }: { label: string }) {
  return (
    <span className="rounded-md border border-white/10 px-1.5 py-0.5 text-[11px] text-[var(--hud-muted)]">
      {label}
    </span>
  );
}

export function ShellView() {
  useHudLifecycle();
  useRuntimeInfo();
  const latencyMs = useAppStore((state) => state.latencyMs);
  const hotkey = useAppStore((state) => state.hotkey);
  const hotkeyLabel = hotkey
    ? `${hotkey} 呼出 / 收起`
    : t("hudHintHotkeyUnavailable");

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-2xl border text-[var(--hud-text)]"
      style={{
        borderColor: "var(--hud-border)",
        background: "var(--hud-surface)",
        boxShadow: "0 18px 48px oklch(0 0 0 / 45%)",
      }}
    >
      {/* 标题栏同时是拖动区 */}
      <header
        data-tauri-drag-region
        className="flex shrink-0 items-center gap-2 border-b border-white/8 px-3 py-2"
      >
        <span
          className="size-2 rounded-full"
          style={{ background: "var(--hud-accent)" }}
        />
        <span className="text-[13px] font-medium tracking-wide">
          {t("appName")}
        </span>
        <span className="text-[11px] text-[var(--hud-muted)]">
          {t("hudTagline")}
        </span>
        {latencyMs !== null && (
          <span className="ml-auto font-mono text-[11px] text-[var(--hud-accent)]">
            {t("legacyLatency")} {latencyMs.toFixed(1)}ms
          </span>
        )}
      </header>

      <main className="flex min-h-0 flex-1 flex-col justify-center gap-2 px-4 py-3">
        <p className="text-[13px] font-medium">{t("hudEmptyTitle")}</p>
        <p className="text-[12px] leading-relaxed text-[var(--hud-muted)]">
          {t("hudEmptyBody")}
        </p>
      </main>

      <footer className="flex shrink-0 items-center gap-2 border-t border-white/8 px-3 py-1.5">
        <Hint label={hotkeyLabel} />
        <Hint label={t("hudHintEscape")} />
        <span className="ml-auto text-[11px] text-[var(--hud-muted)]">
          {t("hudHintSettings")}
        </span>
      </footer>
    </div>
  );
}
