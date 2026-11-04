import { useEffect, useRef, useState } from "react";

import { t } from "../i18n";
import { ipc, isTauri } from "../lib/ipc";
import type { ReaderContent } from "../types";

type BlockState =
  | { status: "loading" }
  | { status: "done"; text: string; provider: string; cached: boolean }
  | { status: "error"; error: string };

/**
 * 双栏对照窗口。
 *
 * - 左原文右译文，块级一一对应（块由 Rust 侧切好，见 `core::reader::split_blocks`）；
 * - 逐块翻译：块小、命中缓存概率高，也不会一个长段把接口打爆；
 * - 联动滚动按百分比同步——不做逐块锚定，简单可靠。
 */
export function ReaderView() {
  const [content, setContent] = useState<ReaderContent | null>(null);
  const [blocks, setBlocks] = useState<BlockState[]>([]);
  const [error, setError] = useState<string | null>(null);

  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const syncing = useRef(false);

  // 拉内容
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .readerContent()
      .then((payload) => {
        if (!payload) {
          setError("没有可对照的内容");
          return;
        }
        setContent(payload);
        setBlocks(payload.blocks.map(() => ({ status: "loading" })));
      })
      .catch((reason: unknown) =>
        setError(reason instanceof Error ? reason.message : String(reason)),
      );
  }, []);

  // 逐块翻译（串行，避免一次打十几个请求）
  useEffect(() => {
    if (!content) {
      return;
    }
    let cancelled = false;

    void (async () => {
      for (let index = 0; index < content.blocks.length; index += 1) {
        if (cancelled) {
          return;
        }
        const block = content.blocks[index]!;
        try {
          const result = await ipc.translateText(block);
          if (cancelled) {
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
          if (cancelled) {
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
    })();

    return () => {
      cancelled = true;
    };
  }, [content]);

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

      {error && (
        <p className="px-4 py-3 text-[12px] text-red-300/80">{error}</p>
      )}

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
              {block}
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
                <p className="text-[13.5px] leading-[1.75]">{block.text}</p>
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
    </div>
  );
}
