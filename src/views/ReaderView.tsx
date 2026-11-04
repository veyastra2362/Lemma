import { useCallback, useEffect, useRef, useState } from "react";

import { t } from "../i18n";
import { ipc, isTauri, onReaderReload } from "../lib/ipc";
import type { Entry, ReaderContent } from "../types";
import { tokenize } from "../utils/tokens";

type BlockState =
  | { status: "loading" }
  | { status: "done"; text: string; provider: string; cached: boolean }
  | { status: "error"; error: string };

/** 点词后弹出的释义面板。 */
interface Picked {
  word: string;
  entry: Entry | null;
}

/**
 * 双栏对照窗口。
 *
 * - 左原文右译文，块级一一对应（块由 Rust 侧切好，见 `core::reader::split_blocks`）；
 * - 逐块串行翻译：块小、命中缓存概率高，也不会把免费接口打限流；
 * - 联动滚动按百分比同步；
 * - **窗口是隐藏复用的**，所以内容必须靠 `reader://reload` 事件刷新，不能只在挂载时拉一次；
 * - 译文里的词可点，弹出该词的词条（中文查中文表，英文查英文表）。
 */
export function ReaderView() {
  const [content, setContent] = useState<ReaderContent | null>(null);
  const [blocks, setBlocks] = useState<BlockState[]>([]);
  const [picked, setPicked] = useState<Picked | null>(null);
  const [error, setError] = useState<string | null>(null);

  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const syncing = useRef(false);
  const loadSeq = useRef(0);

  /** 拉内容 + 逐块翻译。每次收到 reload 都整轮重来。 */
  const load = useCallback(() => {
    if (!isTauri()) {
      return () => undefined;
    }
    const seq = loadSeq.current + 1;
    loadSeq.current = seq;
    let cancelled = false;

    setPicked(null);
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
    return () => {
      cancel();
    };
  }, [load]);

  // Esc 关闭；窗口复用后必须重新拉内容
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void ipc.closeReader();
      }
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
  }, [load]);

  // 联动滚动：任一侧滚动时按百分比同步另一侧
  const syncScroll = (from: "left" | "right") => {
    if (syncing.current) {
      return;
    }
    const source = from === "left" ? leftRef.current : rightRef.current;
    const target = from === "left" ? rightRef.current : leftRef.current;
    if (!source || !target) {
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

  /** 点译文里的词：查词条并弹出面板。 */
  const pickWord = (word: string) => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .lookupWord(word)
      .then((entry) => setPicked({ word, entry }))
      .catch(() => setPicked({ word, entry: null }));
  };

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
              {tokenize(block).map((token, tokenIndex) =>
                token.kind === "word" ? (
                  <button
                    type="button"
                    key={`${tokenIndex}-${token.value}`}
                    onClick={() => pickWord(token.value)}
                    className="rounded-sm underline decoration-dotted decoration-white/15 underline-offset-2 transition hover:bg-white/10 hover:decoration-[var(--hud-accent)]"
                  >
                    {token.value}
                  </button>
                ) : (
                  <span key={`${tokenIndex}-sep`}>{token.value}</span>
                ),
              )}
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
                  {tokenize(block.text).map((token, tokenIndex) =>
                    token.kind === "word" ? (
                      <button
                        type="button"
                        key={`${tokenIndex}-${token.value}`}
                        onClick={() => pickWord(token.value)}
                        className="rounded-sm underline decoration-dotted decoration-white/25 underline-offset-2 transition hover:bg-white/10 hover:decoration-[var(--hud-accent)]"
                      >
                        {token.value}
                      </button>
                    ) : (
                      <span key={`${tokenIndex}-sep`}>{token.value}</span>
                    ),
                  )}
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

      {picked && (
        <footer className="shrink-0 border-t border-white/8 bg-black/20 px-4 py-2">
          <div className="flex items-baseline gap-2">
            <span className="text-[15px] font-medium">{picked.word}</span>
            {picked.entry?.phonetic && (
              <span className="font-mono text-[11px] text-[var(--hud-muted)]">
                /{picked.entry.phonetic}/
              </span>
            )}
            <button
              type="button"
              onClick={() => setPicked(null)}
              className="ml-auto rounded border border-white/12 px-1.5 py-0.5 text-[10px] text-[var(--hud-muted)] hover:border-white/30"
            >
              {t("hudHintEscape")}
            </button>
          </div>
          {picked.entry ? (
            <div className="mt-1 space-y-0.5">
              {picked.entry.translation
                .split("\n")
                .filter((line) => line.trim().length > 0)
                .slice(0, 3)
                .map((line, index) => (
                  <p key={index} className="text-[12px] leading-relaxed">
                    {line}
                  </p>
                ))}
            </div>
          ) : (
            <p className="mt-1 text-[11px] text-[var(--hud-muted)]">
              {t("cardNotInDict")}
            </p>
          )}
        </footer>
      )}
    </div>
  );
}
