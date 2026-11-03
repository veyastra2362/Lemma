import { useHudLifecycle } from "../hooks/useHudLifecycle";
import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t } from "../i18n";
import { useAppStore } from "../store/useAppStore";
import type { Capture, CaptureTier, Settings } from "../types";

function Hint({ label }: { label: string }) {
  return (
    <span className="rounded-md border border-white/10 px-1.5 py-0.5 text-[11px] text-[var(--hud-muted)]">
      {label}
    </span>
  );
}

function triggerLabel(settings: Settings | null): string {
  if (!settings) {
    return t("hudTriggerUnavailable");
  }
  return settings.trigger === "doubleControl"
    ? t("hudTriggerDoubleControl")
    : `${settings.hotkey} ${t("hudTriggerHotkeySuffix")}`;
}

const TIER_LABEL: Record<CaptureTier, string> = {
  selection: "选区",
  word: "光标词",
  clipboard: "剪贴板",
};

function CaptureBody({ capture }: { capture: Capture | null }) {
  if (!capture) {
    return (
      <p className="text-[12px] leading-relaxed text-[var(--hud-muted)]">
        {t("hudCapturePlaceholder")}
      </p>
    );
  }

  if (capture.error) {
    return (
      <>
        <p className="text-[13px] font-medium text-[var(--hud-muted)]">
          {t("hudCaptureNoResult")}
        </p>
        <p className="text-[11px] leading-relaxed text-red-300/80">{capture.error}</p>
      </>
    );
  }

  return (
    <>
      <div className="flex items-baseline gap-2">
        <p className="min-w-0 flex-1 break-words text-[20px] leading-tight font-medium">
          {capture.text}
        </p>
        {capture.tier && (
          <span
            className="shrink-0 rounded px-1.5 py-0.5 text-[10px]"
            style={{
              color: "var(--hud-accent)",
              border: "1px solid color-mix(in oklch, var(--hud-accent) 40%, transparent)",
            }}
          >
            {TIER_LABEL[capture.tier]}
          </span>
        )}
      </div>

      {capture.sentence && capture.sentence !== capture.text && (
        <p className="mt-1 line-clamp-3 text-[12px] leading-relaxed text-[var(--hud-muted)]">
          {capture.sentence}
        </p>
      )}

      <p className="mt-1 font-mono text-[10px] text-[var(--hud-muted)]/70">
        {t("hudCaptureFrom")} {capture.app || "?"} · {capture.elapsedMs.toFixed(0)}ms
      </p>
    </>
  );
}

export function ShellView() {
  useHudLifecycle();
  useRuntimeInfo();
  const latencyMs = useAppStore((state) => state.latencyMs);
  const settings = useAppStore((state) => state.settings);
  const capture = useAppStore((state) => state.capture);

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
            {t("latency")} {latencyMs.toFixed(1)}ms
          </span>
        )}
      </header>

      <main className="flex min-h-0 flex-1 flex-col justify-center gap-1 px-4 py-3">
        <CaptureBody capture={capture} />
      </main>

      <footer className="flex shrink-0 items-center gap-2 border-t border-white/8 px-3 py-1.5">
        <Hint label={triggerLabel(settings)} />
        <Hint label={t("hudHintEscape")} />
        <span className="ml-auto text-[11px] text-[var(--hud-muted)]">
          {t("hudHintSettings")}
        </span>
      </footer>
    </div>
  );
}
