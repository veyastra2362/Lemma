import { useEffect, useRef, useState } from "react";

import { useHudLifecycle } from "../hooks/useHudLifecycle";
import { useRuntimeInfo } from "../hooks/useRuntimeInfo";
import { t, tDeckCollapsed } from "../i18n";
import { ipc, isTauri, onReaderSmoke } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import { shouldTranslate, useDeckStore, type Card } from "../store/useDeckStore";
import type { CardSections, Settings } from "../types";
import { tokenize } from "../utils/tokens";

/** header + footer + 主体上下内边距 */
const CHROME_HEIGHT_PX = 92;
/** 面包屑出现时多出来的高度 */
const BREADCRUMB_HEIGHT_PX = 24;
/** 底部提示栏的高度（隐藏时要从补偿里减掉） */
const FOOTER_HEIGHT_PX = 30;
/** 页眉最小高度 */
const MIN_HEIGHT_PX = 148;
/** 只挂最近几张卡片到 DOM——这是「物理挂载深度限制」的前端部分。 */
const MAX_MOUNTED_CARDS = 3;

/** 复用的小控件样式（写在组件外，避免每处重复一长串 class）。 */
const HUD_CTL_STYLES = `
  .hud-btn {
    font: inherit;
    font-size: 10.5px;
    line-height: 1.45;
    padding: 1px 7px;
    border: 1px solid var(--hud-border);
    border-radius: var(--hud-r-ctl);
    background: transparent;
    color: var(--hud-muted);
    cursor: pointer;
  }
  .hud-btn:hover { color: var(--hud-text); border-color: var(--hud-muted); }
  .hud-ico {
    font: inherit;
    font-size: 11px;
    line-height: 1;
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
  .hud-ico:hover { color: var(--hud-text); border-color: var(--hud-border); }
  .hud-key {
    font-family: var(--hud-mono);
    font-size: 10.5px;
    line-height: 1.5;
    padding: 0 5px;
    border: 1px solid var(--hud-border);
    border-bottom-width: 2px;
    border-radius: 4px;
    color: var(--hud-muted);
    background: var(--hud-raised);
  }
`;

/** 触发方式的键帽文案（底部提示用，短）。 */
function triggerKeyLabel(settings: Settings | null): string {
  if (!settings) {
    return "Ctrl";
  }
  return settings.trigger === "doubleControl" ? "Ctrl Ctrl" : settings.hotkey;
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
            className="border-b border-dashed border-[var(--hud-border)] pb-px hover:border-[var(--hud-muted)] hover:text-[var(--hud-text)]"
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
  onOpenReader,
  onSpeak,
  speakOriginal,
  speakTranslation,
}: {
  card: Card;
  sections: CardSections | null;
  onWord: (word: string) => void;
  collapsed: boolean;
  /** 为 null 表示双栏功能关闭，不显示入口。 */
  onOpenReader: ((text: string) => void) | null;
  /** 为 null 表示朗读不可用（模型未装），不显示入口。 */
  onSpeak: ((text: string) => void) | null;
  /** 是否显示「朗读原文」（设置里可关）。 */
  speakOriginal: boolean;
  /** 是否显示「朗读译文」（设置里可关）。 */
  speakTranslation: boolean;
}) {
  const entry = card.entry;

  // ---- 句卡片：译文为主，原文为辅 ----
  if (card.original) {
    const translation = card.translation;
    return (
      <div className="pb-1">
        {translation.status === "done" && (
          <p key="done" className="fade-in text-[14px] leading-[1.7]">
            {translation.text}
          </p>
        )}
        {translation.status === "loading" && (
          <p key="loading" className="fade-in text-[13px] text-[var(--hud-faint)]">
            {t("translateLoading")}
          </p>
        )}
        {translation.status === "error" && (
          <>
            <p className="text-[13px] text-[var(--hud-faint)]">{t("translateFailed")}</p>
            <p className="mt-0.5 text-[11px] leading-relaxed text-[var(--hud-err)]">
              {translation.error}
            </p>
          </>
        )}
        {!collapsed && (
          <p className="mt-1.5 line-clamp-3 text-[12px] leading-[1.6] text-[var(--hud-muted)]">
            {card.original}
          </p>
        )}
        <div className="mt-1.5 flex items-center gap-2 border-t border-[var(--hud-border-soft)] pt-1.5">
          <p className="font-mono text-[10.5px] text-[var(--hud-faint)] tabular-nums">
            {translation.status === "done" &&
              `${translation.cached ? t("translateCached") : translation.provider} ${translation.elapsedMs.toFixed(0)}ms`}
          </p>
          <div className="ml-auto flex shrink-0 items-center gap-1.5">
            {/* 两个朗读入口：原文与译文分开。读英文原文想听中文译文，就点第二个。 */}
            {onSpeak && speakOriginal && (card.original ?? "").trim().length > 0 && (
              <button type="button" onClick={() => onSpeak(card.original as string)} className="hud-btn">
                {t("speakOriginal")}
              </button>
            )}
            {onSpeak &&
              speakTranslation &&
              card.translation.status === "done" &&
              card.translation.text.trim().length > 0 && (
                <button
                  type="button"
                  onClick={() => onSpeak((card.translation as { text: string }).text)}
                  className="hud-btn"
                >
                  {t("speakTranslation")}
                </button>
              )}
            {onOpenReader && (
              <button
                type="button"
                onClick={() => onOpenReader(card.original as string)}
                className="hud-btn"
              >
                {t("readerOpen")}
              </button>
            )}
          </div>
        </div>
      </div>
    );
  }

  if (collapsed) {
    // 旧卡片压成一行：只留词 + 第一条释义，点击可跳回。
    return (
      <div className="border-b border-[var(--hud-border-soft)] py-1">
        <p className="truncate text-[12px] text-[var(--hud-muted)]">
          <span className="text-[var(--hud-text)]">{card.word}</span>
          {entry?.translation ? ` — ${entry.translation.split("\n")[0]}` : ""}
        </p>
      </div>
    );
  }

  return (
    <div className="pb-1">
      <div className="flex flex-wrap items-baseline gap-x-2">
        <p className="min-w-0 break-words font-[family-name:var(--hud-serif)] text-[19px] leading-tight">
          {card.word}
        </p>
        {entry && sections?.phonetic && entry.phonetic && (
          <span className="font-mono text-[11px] text-[var(--hud-faint)]">
            /{entry.phonetic}/
          </span>
        )}
        {entry?.pos && (
          <span className="shrink-0 border border-[var(--hud-border-soft)] px-1 text-[10.5px] text-[var(--hud-muted)]">
            {entry.pos}
          </span>
        )}
      </div>

      {entry && sections?.translation && entry.translation && (
        <div className="mt-1 space-y-0.5">
          {entry.translation
            .split("\n")
            .filter((line) => line.trim().length > 0)
            .map((line, index) => (
              <p key={`${index}-${line.slice(0, 8)}`} className="text-[13.5px] leading-[1.6]">
                <WordLinks text={line} onWord={onWord} />
              </p>
            ))}
        </div>
      )}

      {entry && sections?.definition && entry.definition && (
        <div className="mt-2 space-y-0.5 border-t border-[var(--hud-border-soft)] pt-2">
          {entry.definition
            .split("\n")
            .filter((line) => line.trim().length > 0)
            .map((line, index) => (
              <p
                key={`def-${index}`}
                className="text-[12px] leading-[1.65] text-[var(--hud-muted)]"
              >
                <WordLinks text={line} onWord={onWord} />
              </p>
            ))}
        </div>
      )}

      {entry && (sections?.exchange || sections?.tags) && (
        <div className="mt-2 flex flex-wrap items-baseline gap-x-3 gap-y-1 text-[11px] text-[var(--hud-faint)]">
          {sections?.exchange && entry.exchange && (
            <span className="font-mono">{entry.exchange}</span>
          )}
          {sections?.tags &&
            entry.tag &&
            entry.tag
              .split(" ")
              .slice(0, 6)
              .map((tag) => <span key={tag}>#{tag}</span>)}
        </div>
      )}

      {/* 中文切出多个词时不猜用户想查哪个，列成候选让他点 */}
      {card.candidates.length > 0 && (
        <div className="mt-2">
          <p className="mb-1 text-[11px] text-[var(--hud-faint)]">{t("cjkCandidates")}</p>
          <div className="flex flex-wrap gap-1.5">
            {card.candidates.map((candidate) => (
              <button
                key={candidate}
                type="button"
                onClick={() => onWord(candidate)}
                className="hud-btn"
              >
                {candidate}
              </button>
            ))}
          </div>
        </div>
      )}

      {!entry && card.candidates.length === 0 && (
        <p className="text-[12px] text-[var(--hud-faint)]">{t("cardNotInDict")}</p>
      )}

      {/* 来源与取词耗时只在标题栏显示一次，这里不再重复 */}
    </div>
  );
}

export function ShellView() {
  useHudLifecycle();
  useRuntimeInfo();

  const capture = useAppStore((state) => state.capture);
  const settings = useAppStore((state) => state.settings);
  const cards = useDeckStore((state) => state.cards);
  const showHints = settings?.showHints ?? true;
  const push = useDeckStore((state) => state.push);
  const jumpTo = useDeckStore((state) => state.jumpTo);
  const setTranslation = useDeckStore((state) => state.setTranslation);
  const [readerError, setReaderError] = useState<string | null>(null);
  const [speechReady, setSpeechReady] = useState(false);
  /** 标题栏显示"这条信息从哪来、多快"，取最底层那张卡。 */
  const rootCard = cards[0] ?? null;

  const contentRef = useRef<HTMLDivElement>(null);
  const lastRequested = useRef(0);

  // 高度自适应：量内容真实高度，交给 Rust 钳制并改窗口尺寸。
  useEffect(() => {
    const element = contentRef.current;
    if (!element || !isTauri()) {
      return;
    }

    const apply = () => {
      // 面板四周各留 8px 透明边距（见 styles.css 的 --hud-gutter），高度回报要把它们算进来
      const GUTTER_PX = 16;
      const chrome =
        CHROME_HEIGHT_PX +
        (cards.length > 1 ? BREADCRUMB_HEIGHT_PX : 0) -
        (showHints ? 0 : FOOTER_HEIGHT_PX) +
        GUTTER_PX;
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
  }, [cards, settings, showHints]);

  // 朗读模型是否就绪（装了就显示朗读按钮）
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .speechReady()
      .then(setSpeechReady)
      .catch(() => setSpeechReady(false));
  }, []);

  // 诊断用：Rust 侧发起的 IPC 冒烟，走与点击按钮完全相同的路径。
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void onReaderSmoke((text) => {
      void ipc.logFrontend("info", "smoke event received, chars=" + text.length);
      void ipc
        .openReader(text, "smoke-ipc")
        .then(() => {
          setReaderError(null);
          void ipc.logFrontend("info", "smoke openReader ok");
        })
        .catch((reason: unknown) => {
          const message = reason instanceof Error ? reason.message : String(reason);
          setReaderError(message);
          void ipc.logFrontend("error", "smoke openReader failed: " + message);
        });
    }).catch((reason: unknown) => {
      void ipc.logFrontend(
        "error",
        "listen(reader://smoke) failed: " + String(reason),
      );
    });
  }, []);

  // 句卡片：一旦出现就去翻译（异步，不挡 HUD 首帧）。
  //
  // 幂等只靠 `translation.status === "idle"` 判断，**不要**再拿卡片 id 做去重：
  // 每次呼出 Deck 都会 reset，根卡片 id 恒为 0，用 id 去重会导致「只有第一次会翻译」。
  const translateCard = cards[0];
  useEffect(() => {
    if (!shouldTranslate(translateCard)) {
      return;
    }
    const cardId = translateCard.id;
    setTranslation(cardId, { status: "loading" });
    void ipc
      .translateText(translateCard.original)
      .then((result) => {
        setTranslation(cardId, {
          status: "done",
          text: result.text,
          provider: result.provider,
          elapsedMs: result.elapsedMs,
          cached: result.cached,
        });
      })
      .catch((reason: unknown) => {
        setTranslation(cardId, {
          status: "error",
          error: reason instanceof Error ? reason.message : String(reason),
        });
      });
  }, [translateCard, setTranslation]);

  const speak = (text: string) => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .speakText(text)
      .then((result) => {
        void ipc.logFrontend(
          "info",
          `speak ok ${result.durationSecs.toFixed(2)}s audio / ${result.synthesisMs.toFixed(0)}ms`,
        );
      })
      .catch((reason: unknown) => {
        void ipc.logFrontend("error", "speak failed: " + String(reason));
      });
  };

  const openReader = (text: string) => {
    if (!isTauri()) {
      return;
    }
    // 失败必须让用户看见：这个功能之前就是被 .catch(() => undefined) 悄悄吞掉的。
    void ipc
      .openReader(text, capture?.app ?? "")
      .then(() => setReaderError(null))
      .catch((reason: unknown) => {
        setReaderError(reason instanceof Error ? reason.message : String(reason));
      });
  };

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
    <div className="lm-panel flex flex-col text-[var(--hud-text)]">
      <style>{HUD_CTL_STYLES}</style>
      {/* 标题栏只放"这条信息从哪来、多快"，开发用的性能与标语挪去日志 */}
      <header className="flex shrink-0 items-center gap-2 border-b border-[var(--hud-border-soft)] px-3 py-1.5">
        <span className="text-[11.5px] text-[var(--hud-muted)]">
          {rootCard?.source ?? t("appName")}
        </span>
        {rootCard?.elapsedMs != null && (
          <span className="font-mono text-[11px] tabular-nums text-[var(--hud-faint)]">
            {rootCard.elapsedMs.toFixed(0)} ms
          </span>
        )}
        <span className="ml-auto flex items-center gap-0.5">
          <button
            type="button"
            aria-label={t("settingsTitle")}
            title={t("settingsTitle")}
            onClick={() => {
              void ipc.openSettings().catch((reason: unknown) => {
                void ipc.logFrontend("error", "openSettings failed: " + String(reason));
              });
            }}
            className="hud-ico"
          >
            ⚙
          </button>
          <button
            type="button"
            aria-label={t("hudHintEscape")}
            title={t("hudHintEscape")}
            onClick={() => {
              void ipc.hudHide().catch(() => undefined);
            }}
            className="hud-ico"
          >
            ✕
          </button>
        </span>
      </header>

      {cards.length > 1 && (
        <nav className="flex shrink-0 flex-wrap items-center gap-x-1 border-b border-[var(--hud-border-soft)] px-3 py-1 text-[11px] text-[var(--hud-faint)]">
          {cards.map((card, index) => (
            <span key={card.id} className="flex items-center gap-1">
              {index > 0 && <span className="opacity-50">›</span>}
              <button
                type="button"
                onClick={() => jumpTo(index)}
                className={
                  index === cards.length - 1
                    ? "text-[var(--hud-accent)]"
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
              <p className="border-b border-[var(--hud-border-soft)] pb-1 text-[11px] text-[var(--hud-faint)]">
                {tDeckCollapsed(collapsedUntil)}
              </p>
            )}
            {mounted.map((card, index) => (
              <div
                // 最上面那张（刚下探出来的）做一次 2px 上移淡入；旧卡片不动
                key={card.id}
                className={index === mounted.length - 1 ? "rise-in" : undefined}
              >
                <CardView
                  card={card}
                  sections={settings?.cardSections ?? null}
                  onWord={openWord}
                  collapsed={index < mounted.length - 1}
                  onOpenReader={
                    // 双栏只给截图结果：选词时 HUD 已经并排显示原文与译文，再开窗口是多余的
                    settings?.readerOpenMode !== "off" && card.screenshot ? openReader : null
                  }
                  onSpeak={speechReady ? speak : null}
                  speakOriginal={settings?.speakOriginal ?? true}
                  speakTranslation={settings?.speakTranslation ?? true}
                />
              </div>
            ))}
          </>
        )}
          {readerError && (
            <p className="mt-1 rounded border border-red-400/30 bg-red-500/10 px-2 py-1 text-[11px] text-red-300">
              {t("readerFailed")}：{readerError}
            </p>
          )}
        </div>
      </main>

      {showHints && (
        <footer className="flex shrink-0 items-center gap-1.5 border-t border-[var(--hud-border-soft)] px-3 py-1.5">
          <span className="hud-key">{triggerKeyLabel(settings)}</span>
          <span className="text-[10.5px] text-[var(--hud-faint)]">{t("hudHintTrigger")}</span>
          <span className="hud-key ml-2">Esc</span>
          <span className="text-[10.5px] text-[var(--hud-faint)]">{t("hudHintEscapeShort")}</span>
        </footer>
      )}
    </div>
  );
}
