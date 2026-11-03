import { useEffect, useRef } from "react";

import { useHudLifecycle } from "../hooks/useHudLifecycle";
import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t, tDeckCollapsed } from "../i18n";
import { ipc, isTauri } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import { useDeckStore, type Card } from "../store/useDeckStore";
import type { CardSections, Settings } from "../types";
import { tokenize } from "../utils/tokens";

/** header + footer + 主体上下内边距 */
const CHROME_HEIGHT_PX = 92;
/** 面包屑出现时多出来的高度 */
const BREADCRUMB_HEIGHT_PX = 24;
/** 页眉最小高度 */
const MIN_HEIGHT_PX = 132;
/** 只挂最近几张卡片到 DOM——这是「物理挂载深度限制」的前端部分。 */
const MAX_MOUNTED_CARDS = 3;

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

/** 释义里可点的词。悬停下划虚线，让"能点"这件事一眼可见。 */
function WordLinks({
  text,
  onWord,
}: {
  text: string;
  onWord: (word: string) => void;
}) {
  return (
    <>
      {tokenize(text).map((token, index) =>
        token.kind === "word" ? (
          <button
            type="button"
            key={`${index}-${token.value}`}
            onClick={() => onWord(token.value)}
            className="rounded-sm underline decoration-dotted decoration-white/25 underline-offset-2 hover:bg-white/10 hover:decoration-[var(--hud-accent)]"
          >
            {token.value}
          </button>
        ) : (
          <span key={`${index}-sep`}>{token.value}</span>
        ),
      )}
    </>
  );
}

function CardView({
  card,
  sections,
  onWord,
  collapsed,
}: {
  card: Card;
  sections: CardSections | null;
  onWord: (word: string) => void;
  collapsed: boolean;
}) {
  const entry = card.entry;

  if (collapsed) {
    // 旧卡片压成一行：只留词 + 第一条释义，点击可跳回。
    return (
      <div className="border-b border-white/6 py-1">
        <p className="truncate text-[13px] text-[var(--hud-muted)]">
          <span className="text-[var(--hud-text)]/70">{card.word}</span>
          {entry?.translation ? ` — ${entry.translation.split("\n")[0]}` : ""}
        </p>
      </div>
    );
  }

  return (
    <div className="pb-1">
      <div className="flex items-baseline gap-2">
        <p className="min-w-0 flex-1 break-words text-[20px] leading-tight font-medium">
          {card.word}
        </p>
        {card.source && (
          <span
            className="shrink-0 rounded px-1.5 py-0.5 text-[10px]"
            style={{
              color: "var(--hud-accent)",
              border: "1px solid color-mix(in oklch, var(--hud-accent) 40%, transparent)",
            }}
          >
            {card.source}
          </span>
        )}
      </div>

      {entry && sections?.phonetic && entry.phonetic && (
        <p className="font-mono text-[12px] text-[var(--hud-muted)]">
          /{entry.phonetic}/
        </p>
      )}

      {entry && sections?.translation && entry.translation && (
        <div className="mt-0.5 space-y-0.5">
          {entry.translation
            .split("\n")
            .filter((line) => line.trim().length > 0)
            .map((line, index) => (
              <p key={`${index}-${line.slice(0, 8)}`} className="text-[13px] leading-relaxed">
                <WordLinks text={line} onWord={onWord} />
              </p>
            ))}
        </div>
      )}

      {entry && sections?.definition && entry.definition && (
        <div className="mt-1 space-y-0.5">
          {entry.definition
            .split("\n")
            .filter((line) => line.trim().length > 0)
            .map((line, index) => (
              <p
                key={`def-${index}`}
                className="text-[12px] leading-relaxed text-[var(--hud-muted)]"
              >
                <WordLinks text={line} onWord={onWord} />
              </p>
            ))}
        </div>
      )}

      {entry && sections?.exchange && entry.exchange && (
        <p className="mt-1 font-mono text-[11px] text-[var(--hud-muted)]">
          {entry.exchange}
        </p>
      )}

      {entry && sections?.tags && entry.tag && (
        <div className="mt-1 flex flex-wrap gap-1">
          {entry.tag.split(" ").slice(0, 6).map((tag) => (
            <span
              key={tag}
              className="rounded border border-white/10 px-1 py-0.5 text-[10px] text-[var(--hud-muted)]"
            >
              {tag}
            </span>
          ))}
        </div>
      )}

      {!entry && (
        <p className="text-[12px] text-[var(--hud-muted)]">{t("cardNotInDict")}</p>
      )}

      {card.elapsedMs !== null && (
        <p className="mt-1 font-mono text-[10px] text-[var(--hud-muted)]/70">
          {card.source ?? ""} · {card.elapsedMs.toFixed(0)}ms
        </p>
      )}
    </div>
  );
}

export function ShellView() {
  useHudLifecycle();
  useRuntimeInfo();

  const capture = useAppStore((state) => state.capture);
  const settings = useAppStore((state) => state.settings);
  const latencyMs = useAppStore((state) => state.latencyMs);
  const cards = useDeckStore((state) => state.cards);
  const push = useDeckStore((state) => state.push);
  const jumpTo = useDeckStore((state) => state.jumpTo);

  const contentRef = useRef<HTMLDivElement>(null);
  const lastRequested = useRef(0);

  // 高度自适应：量内容真实高度，交给 Rust 钳制并改窗口尺寸。
  useEffect(() => {
    const element = contentRef.current;
    if (!element || !isTauri()) {
      return;
    }

    const apply = () => {
            const chrome =
        CHROME_HEIGHT_PX + (cards.length > 1 ? BREADCRUMB_HEIGHT_PX : 0);
      const desired = Math.max(
        MIN_HEIGHT_PX,
        Math.ceil(element.scrollHeight + chrome),
      );
      // 避免和窗口尺寸变化互相触发成死循环。
      if (Math.abs(desired - lastRequested.current) < 2) {
        return;
      }
      lastRequested.current = desired;
      void ipc.resizeHud(desired).catch(() => undefined);
    };

    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(element);
    return () => observer.disconnect();
  }, [cards, settings]);

  const openWord = (word: string) => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .lookupWord(word)
      .then((entry) => {
        push(word, entry);
      })
      .catch(() => undefined);
  };

  const mounted = cards.slice(-MAX_MOUNTED_CARDS);
  const collapsedUntil = cards.length - mounted.length;

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-2xl border text-[var(--hud-text)]"
      style={{
        borderColor: "var(--hud-border)",
        background: "var(--hud-surface)",
        boxShadow: "0 18px 48px oklch(0 0 0 / 45%)",
      }}
    >
      <header
        data-tauri-drag-region
        className="flex shrink-0 items-center gap-2 border-b border-white/8 px-3 py-2"
      >
        <span
          className="size-2 rounded-full"
          style={{ background: "var(--hud-accent)" }}
        />
        <span className="text-[13px] font-medium tracking-wide">{t("appName")}</span>
        <span className="text-[11px] text-[var(--hud-muted)]">{t("hudTagline")}</span>
        {latencyMs !== null && (
          <span className="ml-auto font-mono text-[11px] text-[var(--hud-accent)]">
            {t("latency")} {latencyMs.toFixed(1)}ms
          </span>
        )}
      </header>

      {cards.length > 1 && (
        <nav className="flex shrink-0 flex-wrap items-center gap-x-1 gap-y-0.5 border-b border-white/8 px-3 py-1 text-[11px] text-[var(--hud-muted)]">
          {cards.map((card, index) => (
            <span key={card.id} className="flex items-center gap-1">
              {index > 0 && <span className="opacity-40">›</span>}
              <button
                type="button"
                onClick={() => jumpTo(index)}
                className={
                  index === cards.length - 1
                    ? "text-[var(--hud-text)]"
                    : "hover:text-[var(--hud-text)]"
                }
              >
                {card.word}
              </button>
            </span>
          ))}
        </nav>
      )}

      {/* 量高度的必须是内层"内容"div：外层是滚动容器，内容不足时它的
          scrollHeight 等于容器高度，会让"算高度 → 改窗口 → 再算"形成自增循环。 */}
      <main className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        <div ref={contentRef}>
        {cards.length === 0 ? (
          <p className="text-[12px] leading-relaxed text-[var(--hud-muted)]">
            {capture?.error ? capture.error : t("hudCapturePlaceholder")}
          </p>
        ) : (
          <>
            {collapsedUntil > 0 && (
              <p className="border-b border-white/6 pb-1 text-[11px] text-[var(--hud-muted)]/60">
                {tDeckCollapsed(collapsedUntil)}
              </p>
            )}
            {mounted.map((card, index) => (
              <CardView
                key={card.id}
                card={card}
                sections={settings?.cardSections ?? null}
                onWord={openWord}
                collapsed={index < mounted.length - 1}
              />
            ))}
          </>
        )}
        </div>
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
