import { useCallback, useEffect, useRef, useState } from "react";

import { t } from "../i18n";
import { ipc, isTauri, onReaderReload } from "../lib/ipc";
import type { Entry, ReaderBlock, ReaderContent, ReaderImage } from "../types";
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

/** 显示形态：并排（左图右译）或覆盖（译文贴在原图框上）。 */
type ViewMode = "side" | "overlay";

const MAX_DRILL = 16;

/**
 * 双栏对照窗口。
 *
 * 两种形态：
 * - **图文模式**（截图 OCR）：左原图 + 右译文，或把译文覆盖在原图对应文本框上。
 *   框的位置来自 OCR 的 `quad`，所以译文和图上文字是**逐块对齐**的。
 * - **纯文本模式**（选中一段文字）：左原文右译文，按段落块对齐。
 *
 * 两栏都可点词，点中入下探栈（与 HUD 的 Deck 同构），`Esc` 逐层退。
 * 窗口是隐藏复用的：内容靠 `reader://reload` 刷新，阅读位置按文本记忆。
 */
export function ReaderView() {
  const [content, setContent] = useState<ReaderContent | null>(null);
  const [blocks, setBlocks] = useState<BlockState[]>([]);
  const [drill, setDrill] = useState<Drill[]>([]);
  const [mode, setMode] = useState<ViewMode>("side");
  const [hovered, setHovered] = useState<number | null>(null);
  const [speechReady, setSpeechReady] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const syncing = useRef(false);
  const loadSeq = useRef(0);
  const scrollMemory = useRef<{ text: string; left: number; right: number } | null>(null);

  const isImage = Boolean(content?.image && content.imageBlocks.length > 0);
  const sources = isImage
    ? (content?.imageBlocks ?? []).map((block: ReaderBlock) => block.text)
    : (content?.blocks ?? []);

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

        const texts = payload.imageBlocks.length > 0
          ? payload.imageBlocks.map((block) => block.text)
          : payload.blocks;
        setBlocks(texts.map(() => ({ status: "loading" })));

        if (scrollMemory.current?.text !== payload.text) {
          scrollMemory.current = { text: payload.text, left: 0, right: 0 };
        }

        for (let index = 0; index < texts.length; index += 1) {
          if (cancelled || seq !== loadSeq.current) {
            return;
          }
          try {
            const result = await ipc.translateText(texts[index]!);
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

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
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

  /** 译文一栏：并排与覆盖两种形态共用。 */
  const renderTranslation = (index: number, compact: boolean) => {
    const block = blocks[index];
    if (!block) {
      return null;
    }
    if (block.status === "loading") {
      return (
        <p className="text-[12px] text-[var(--hud-muted)]">{t("translateLoading")}</p>
      );
    }
    if (block.status === "error") {
      return <p className="text-[11px] leading-relaxed text-red-300/80">{block.error}</p>;
    }
    return (
      <p className={compact ? "text-[12px] leading-[1.6]" : "text-[13.5px] leading-[1.75]"}>
        {renderClickable(block.text, false)}
      </p>
    );
  };

  const imageSrc = content?.image
    ? `data:image/png;base64,${(content.image as ReaderImage).pngBase64}`
    : null;

  /** 把图片坐标换成 CSS 百分比，覆盖层才能跟着缩放。 */
  const quadStyle = (block: ReaderBlock) => {
    const image = content?.image;
    if (!image) {
      return {};
    }
    const xs = block.quad.map(([x]) => x);
    const ys = block.quad.map(([, y]) => y);
    const left = (Math.min(...xs) / image.width) * 100;
    const top = (Math.min(...ys) / image.height) * 100;
    const width = ((Math.max(...xs) - Math.min(...xs)) / image.width) * 100;
    const height = ((Math.max(...ys) - Math.min(...ys)) / image.height) * 100;
    return { left: `${left}%`, top: `${top}%`, width: `${width}%`, height: `${height}%` };
  };

  return (
    <div
      className="flex h-full w-full flex-col overflow-hidden rounded-xl border text-[var(--hud-text)]"
      style={{ borderColor: "var(--hud-border)", background: "var(--hud-surface)" }}
    >
      {/* 只有标题文字可拖拽：整块 header 挂 drag-region 会把按钮的点击吞掉（实测） */}
      <header className="flex shrink-0 items-center gap-2 border-b border-white/8 px-4 py-2.5">
        <span data-tauri-drag-region className="text-[13px] font-medium">
          {t("readerTitle")}
        </span>
        {content?.app && (
          <span className="text-[11px] text-[var(--hud-muted)]">{content.app}</span>
        )}
        {isImage && (
          <div className="ml-2 flex overflow-hidden rounded-md border border-white/12">
            {(["side", "overlay"] as const).map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => setMode(option)}
                className={`px-2 py-0.5 text-[11px] transition ${
                  mode === option
                    ? "bg-[var(--hud-accent)]/20 text-[var(--hud-accent)]"
                    : "text-[var(--hud-muted)] hover:text-white"
                }`}
              >
                {option === "side" ? t("readerModeSide") : t("readerModeOverlay")}
              </button>
            ))}
          </div>
        )}
        {speechReady && /[\u4e00-\u9fff]/.test(content?.text ?? "") && (
          <button
            type="button"
            onClick={() => {
              void ipc
                .speakText(content?.text ?? "")
                .catch((reason: unknown) => {
                  void ipc.logFrontend("error", "reader speak failed: " + String(reason));
                });
            }}
            className="ml-2 rounded border border-white/12 px-1.5 py-0.5 text-[11px] text-[var(--hud-muted)] transition hover:border-[var(--hud-accent)] hover:text-[var(--hud-accent)]"
          >
            {t("speakStart")}
          </button>
        )}
        <span className="ml-auto text-[11px] text-[var(--hud-muted)]">
          {sources.length} {t("readerBlocks")}
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

      {/* ---------- 覆盖模式：译文贴在原图框上 ---------- */}
      {isImage && mode === "overlay" && imageSrc && (
        <div className="min-h-0 flex-1 overflow-auto p-4">
          <div className="relative mx-auto w-fit" style={{ maxWidth: content!.image!.width }}>
            <img src={imageSrc} alt="" className="w-full rounded-lg opacity-60" />
            {content!.imageBlocks.map((block, index) => (
              <div
                key={index}
                onMouseEnter={() => setHovered(index)}
                onMouseLeave={() => setHovered(null)}
                className={`absolute overflow-hidden rounded border px-1 ${
                  hovered === index
                    ? "border-[var(--hud-accent)] bg-[var(--hud-surface)]/95"
                    : "border-white/10 bg-[var(--hud-surface)]/85"
                }`}
                style={quadStyle(block)}
              >
                {renderTranslation(index, true)}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* ---------- 并排模式 ---------- */}
      {(!isImage || mode === "side") && (
        <div className="grid min-h-0 flex-1 grid-cols-2 divide-x divide-white/8">
          <div
            ref={leftRef}
            onScroll={() => syncScroll("left")}
            className="min-h-0 overflow-y-auto px-4 py-3"
          >
            {isImage && imageSrc ? (
              <div className="space-y-2">
                {/* 不放大：放大只会更糊，超过原尺寸没有意义 */}
                <img
                  src={imageSrc}
                  alt=""
                  className="w-full rounded-lg"
                  style={{ maxWidth: content!.image!.width }}
                />
                {content!.imageBlocks.map((block, index) => (
                  <p
                    key={index}
                    onMouseEnter={() => setHovered(index)}
                    onMouseLeave={() => setHovered(null)}
                    className={`rounded px-1 text-[12px] leading-relaxed transition ${
                      hovered === index
                        ? "bg-[var(--hud-accent)]/15 text-white"
                        : "text-[var(--hud-muted)]"
                    }`}
                  >
                    {block.text}
                  </p>
                ))}
              </div>
            ) : (
              (content?.blocks ?? []).map((block, index) => (
                <p
                  key={index}
                  className="mb-3 text-[13.5px] leading-[1.75] text-[var(--hud-text)]/90"
                >
                  {renderClickable(block, true)}
                </p>
              ))
            )}
          </div>

          <div
            ref={rightRef}
            onScroll={() => syncScroll("right")}
            className="min-h-0 overflow-y-auto px-4 py-3"
          >
            {sources.map((_, index) => (
              <div
                key={index}
                onMouseEnter={() => setHovered(index)}
                onMouseLeave={() => setHovered(null)}
                className={`mb-3 min-h-[1.75em] rounded px-1 transition ${
                  hovered === index ? "bg-[var(--hud-accent)]/10" : ""
                }`}
              >
                {renderTranslation(index, false)}
              </div>
            ))}
          </div>
        </div>
      )}

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
                    index === drill.length - 1 ? "text-[var(--hud-text)]" : "hover:text-white"
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
