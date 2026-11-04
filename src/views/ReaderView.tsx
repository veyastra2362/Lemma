import { useCallback, useEffect, useRef, useState } from "react";

import { t } from "../i18n";
import { ipc, isTauri, onReaderReload } from "../lib/ipc";
import type { Entry, ReaderContent } from "../types";
import { tokenize } from "../utils/tokens";

type BlockState =
  | { status: "loading" }
  | { status: "done"; text: string; provider: string; cached: boolean }
  | { status: "error"; error: string };

/** 下探栈里的一层。 */
interface Drill {
  word: string;
  entry: Entry | null;
}

const MAX_DRILL = 16;

/**
 * 双栏对照窗口（深读工作台）。
 *
 * - 左原文右译文，块级一一对应（块由 Rust 侧 `core::reader::split_blocks` 切好）；
 * - 逐块串行翻译：块小、命中缓存概率高，也不会把免费接口打限流；
 * - 两侧滚动按百分比联动；
 * - **两栏文本都可点**：点词入下探栈，栈里的释义还能继续点（与 HUD 的 Deck 同构），`Esc` 逐层退；
 * - **窗口是隐藏复用的**：内容靠 `reader://reload` 刷新，阅读位置按文本内容记忆。
 */
export function ReaderView() {
  const [content, setContent] = useState<ReaderContent | null>(null);
  const [blocks, setBlocks] = useState<BlockState[]>([]);
  const [drill, setDrill] = useState<Drill[]>([]);
  const [error, setError] = useState<string | null>(null);

  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const syncing = useRef(false);
  const loadSeq = useRef(0);
  /** 阅读位置记忆：同一段文本再次打开回到原处，换了文本就从头开始。 */
  const scrollMemory = useRef<{ text: string; left: number; right: number } | null>(null);

  const load = useCallback(() => {
    if (!isTauri()) {
      return () => undefined;
    }
    const seq = loadSeq.current + 1;
    loadSeq.current = seq;
    let cancelled = false;

    setDrill([]);
    setError(null);

    void ipc
      .readerContent()
      .then(async (payload) => {
        if (cancelled || seq !== loadSeq.current) {
          return;
        }
        if (!payload) {
          setError(t("readerEmpty"));
          return;
        }
        setContent(payload);
        setBlocks(payload.blocks.map(() => ({ status: "loading" })));

        if (scrollMemory.current?.text !== payload.text) {
          scrollMemory.current = { text: payload.text, left: 0, right: 0 };
        }

        for (let index = 0; index < payload.blocks.length; index += 1) {
          if (cancelled || seq !== loadSeq.current) {
            return;
          }
          const block = payload.blocks[index]!;
          try {
            const result = await ipc.translateText(block);
            if (cancelled || seq !== loadSeq.current) {
              return;
            }
            setBlocks((prev) => {
              const next = [...prev];
              next[index] = {
                status: "done",
                text: result.text,
                provider: result.provider,
                cached: result.cached,
              };
              return next;
            });
          } catch (reason) {
            if (cancelled || seq !== loadSeq.current) {
              return;
            }
            setBlocks((prev) => {
              const next = [...prev];
              next[index] = {
                status: "error",
                error: reason instanceof Error ? reason.message : String(reason),
              };
              return next;
            });
          }
        }
      })
      .catch((reason: unknown) => {
        if (!cancelled && seq === loadSeq.current) {
          setError(reason instanceof Error ? reason.message : String(reason));
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const cancel = load();
    return () => cancel();
  }, [load]);

  // 恢复阅读位置（等布局稳定后再设）
  useEffect(() => {
    const memory = scrollMemory.current;
    const left = leftRef.current;
    const right = rightRef.current;
    if (!memory || !left || !right) {
      return;
    }
    const timer = window.requestAnimationFrame(() => {
      left.scrollTop = memory.left;
      right.scrollTop = memory.right;
    });
    return () => window.cancelAnimationFrame(timer);
  }, [content]);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      // 先退下探栈；栈空才关窗口。
      if (drill.length > 0) {
        setDrill((prev) => prev.slice(0, -1));
        return;
      }
      void ipc.closeReader();
    };
    window.addEventListener("keydown", onKeyDown);

    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onReaderReload(() => {
      void ipc.logFrontend("info", "reader reload received");
      load();
    })
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [drill.length, load]);

  const syncScroll = (from: "left" | "right") => {
    const source = from === "left" ? leftRef.current : rightRef.current;
    const target = from === "left" ? rightRef.current : leftRef.current;
    if (!source || !target) {
      return;
    }
    if (scrollMemory.current) {
      scrollMemory.current[from] = source.scrollTop;
    }
    if (syncing.current) {
      return;
    }
    const sourceMax = source.scrollHeight - source.clientHeight;
    const targetMax = target.scrollHeight - target.clientHeight;
    if (sourceMax <= 0 || targetMax <= 0) {
      return;
    }
    syncing.current = true;
    target.scrollTop = (source.scrollTop / sourceMax) * targetMax;
    window.requestAnimationFrame(() => {
      syncing.current = false;
    });
  };

  /** 点词：查词条并压入下探栈。 */
  const pushWord = (word: string) => {
    const term = word.trim();
    if (!isTauri() || !term) {
      return;
    }
    setDrill((prev) => {
      if (prev.length >= MAX_DRILL || prev.some((item) => item.word === term)) {
        return prev;
      }
      return [...prev, { word: term, entry: null }];
    });
    void ipc
      .lookupWord(term)
      .then((entry) => {
        setDrill((prev) =>
          prev.map((item) => (item.word === term ? { ...item, entry } : item)),
        );
      })
      .catch(() => undefined);
  };

  const current = drill[drill.length - 1];

  const renderClickable = (text: string, muted: boolean) => (
    <>
      {tokenize(text).map((token, index) =>
        token.kind === "word" ? (
          <button
            type="button"
            key={`${index}-${token.value}`}
            onClick={() => pushWord(token.value)}
            className={`rounded-sm underline decoration-dotted underline-offset-2 transition hover:bg-white/10 hover:decoration-[var(--hud-accent)] ${
              muted ? "decoration-white/15" : "decoration-white/25"
            }`}
          >
            {token.value}
          </button>
        ) : (
          <span key={`${index}-sep`}>{token.value}</span>
        ),
      )}
    </>
  );

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-xl border text-[var(--hud-text)]"
      style={{ borderColor: "var(--hud-border)", background: "var(--hud-surface)" }}
    >
      <header
        data-tauri-drag-region
        className="flex shrink-0 items-center gap-2 border-b border-white/8 px-4 py-2.5"
      >
        <span className="text-[13px] font-medium">{t("readerTitle")}</span>
        {content?.app && (
          <span className="text-[11px] text-[var(--hud-muted)]">{content.app}</span>
        )}
        <span className="ml-auto text-[11px] text-[var(--hud-muted)]">
          {content ? `${content.blocks.length} ${t("readerBlocks")}` : ""}
        </span>
        <button
          type="button"
          onClick={() => void ipc.closeReader()}
          aria-label={t("settingsClose")}
          className="grid size-6 place-items-center rounded-md text-[var(--hud-muted)] transition hover:bg-white/10 hover:text-[var(--hud-text)]"
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

      {error && <p className="px-4 py-3 text-[12px] text-red-300/80">{error}</p>}

      <div className="grid min-h-0 flex-1 grid-cols-2 divide-x divide-white/8">
        <div
          ref={leftRef}
          onScroll={() => syncScroll("left")}
          className="min-h-0 overflow-y-auto px-4 py-3"
        >
          {content?.blocks.map((block, index) => (
            <p
              key={index}
              className="mb-3 text-[13.5px] leading-[1.75] text-[var(--hud-text)]/90"
            >
              {renderClickable(block, true)}
            </p>
          ))}
        </div>

        <div
          ref={rightRef}
          onScroll={() => syncScroll("right")}
          className="min-h-0 overflow-y-auto px-4 py-3"
        >
          {blocks.map((block, index) => (
            <div key={index} className="mb-3 min-h-[1.75em]">
              {block.status === "done" && (
                <p className="text-[13.5px] leading-[1.75]">
                  {renderClickable(block.text, false)}
                </p>
              )}
              {block.status === "loading" && (
                <p className="text-[12px] text-[var(--hud-muted)]">
                  {t("translateLoading")}
                </p>
              )}
              {block.status === "error" && (
                <p className="text-[11px] leading-relaxed text-red-300/80">
                  {block.error}
                </p>
              )}
            </div>
          ))}
        </div>
      </div>

      {current && (
        <footer className="shrink-0 border-t border-white/8 bg-black/20 px-4 py-2">
          <div className="flex flex-wrap items-center gap-1 text-[11px] text-[var(--hud-muted)]">
            {drill.map((item, index) => (
              <span key={`${item.word}-${index}`} className="flex items-center gap-1">
                {index > 0 && <span className="opacity-40">›</span>}
                <button
                  type="button"
                  onClick={() => setDrill((prev) => prev.slice(0, index + 1))}
                  className={
                    index === drill.length - 1
                      ? "text-[var(--hud-text)]"
                      : "hover:text-white"
                  }
                >
                  {item.word}
                </button>
              </span>
            ))}
            <span className="ml-auto opacity-70">{t("readerEscHint")}</span>
          </div>

          {current.entry?.phonetic && (
            <p className="mt-1 font-mono text-[11px] text-[var(--hud-muted)]">
              /{current.entry.phonetic}/
            </p>
          )}

          {current.entry ? (
            <div className="mt-0.5 max-h-32 space-y-0.5 overflow-y-auto">
              {current.entry.translation
                .split("\n")
                .filter((line) => line.trim().length > 0)
                .slice(0, 5)
                .map((line, index) => (
                  <p key={index} className="text-[12px] leading-relaxed">
                    {renderClickable(line, true)}
                  </p>
                ))}
            </div>
          ) : (
            <p className="mt-0.5 text-[11px] text-[var(--hud-muted)]">
              {t("cardNotInDict")}
            </p>
          )}
        </footer>
      )}
    </div>
  );
}
