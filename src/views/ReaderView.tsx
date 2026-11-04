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
  /** 跟读：正在朗读的词（全局序号），null 表示没在朗读。 */
  const [spokenWord, setSpokenWord] = useState<number | null>(null);
  const speakTimer = useRef<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const imageBoxRef = useRef<HTMLDivElement>(null);
  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const syncing = useRef(false);
  const loadSeq = useRef(0);
  const scrollMemory = useRef<{ text: string; left: number; right: number } | null>(null);

  const isImage = Boolean(content?.image && content.imageBlocks.length > 0);
  const sources = isImage
    ? (content?.imageBlocks ?? []).map((block: ReaderBlock) => block.text)
    : (content?.blocks ?? []);

  /** 全文的词序号表：每个词占多少字符，用于按时间线性插值。 */
  const wordTimeline = (() => {
    const list: number[] = []; // 每个词的字符数
    for (const source of sources) {
      for (const token of tokenize(source)) {
        if (token.kind === "word") {
          list.push(token.value.length);
        }
      }
    }
    return list;
  })();

  const stopSpeaking = useCallback(() => {
    if (speakTimer.current !== null) {
      window.clearTimeout(speakTimer.current);
      speakTimer.current = null;
    }
    setSpokenWord(null);
    void ipc.stopSpeaking();
  }, []);

  /**
   * 跟读高亮。
   *
   * `PlaySound` 不汇报播放位置，所以这里按**字符权重**把总时长线性摊到每个词上：
   * 长词占的时间多、短词少。顺序与合成时的音素顺序一致（同一段文本、同一个分词）。
   * 这是估算而不是真同步，足够让视线跟着走；精确对齐需要换能报位置的播放后端。
   */
  const startSpeaking = useCallback(
    (text: string) => {
      if (speakTimer.current !== null) {
        window.clearTimeout(speakTimer.current);
        speakTimer.current = null;
      }
      setSpokenWord(null);
      void ipc
        .speakText(text)
        .then((result) => {
          const total = wordTimeline.reduce((sum, size) => sum + size, 0) || 1;
          let elapsed = 0;
          let index = 0;
          const step = () => {
            if (index >= wordTimeline.length) {
              setSpokenWord(null);
              speakTimer.current = null;
              return;
            }
            setSpokenWord(index);
            const share = ((wordTimeline[index] ?? 1) / total) * result.durationSecs * 1000;
            elapsed += share;
            index += 1;
            speakTimer.current = window.setTimeout(step, Math.max(share, 60));
          };
          step();
          void ipc.logFrontend(
            "info",
            `speak ${result.durationSecs.toFixed(2)}s words=${wordTimeline.length} budget=${elapsed.toFixed(0)}ms`,
          );
        })
        .catch((reason: unknown) => {
          void ipc.logFrontend("error", "reader speak failed: " + String(reason));
        });
    },
    [wordTimeline],
  );

  // 组件卸载时清掉定时器
  useEffect(
    () => () => {
      if (speakTimer.current !== null) {
        window.clearTimeout(speakTimer.current);
      }
    },
    [],
  );

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

  const renderClickable = (text: string, muted: boolean, baseWord = -1) => {
    // baseWord >= 0 时把该段之前的累计词数加上，得到"全文第几个词"，用于跟读高亮
    let seen = 0;
    return (
      <>
        {tokenize(text).map((token, index) => {
          const wordIndex = token.kind === "word" ? baseWord + seen++ : -1;
          const isSpoken = baseWord >= 0 && wordIndex === spokenWord;
          return token.kind === "word" ? (
            <button
              type="button"
              key={`${index}-${token.value}`}
              onClick={() => pushWord(token.value)}
              className={`rounded-sm underline decoration-dotted underline-offset-2 transition hover:bg-white/10 hover:decoration-[var(--hud-accent)] ${
                muted ? "decoration-white/15" : "decoration-white/25"
              } ${isSpoken ? "bg-[var(--hud-accent)]/35 text-white no-underline" : ""}`}
            >
              {token.value}
            </button>
          ) : (
            <span key={`${index}-sep`}>{token.value}</span>
          );
        })}
      </>
    );
  };

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

  /**
   * 气泡位置：优先放在框**上方**，上方放不下就翻到下方，再左右夹紧在图片内。
   *
   * 为什么不直接盖在原框上：译文通常比原文长，盖住就等于把原文遮了；
   * 浮在旁边既能同时看到原文与译文，也不会像原位格子那样被裁掉。
   */
  const bubbleFor = (index: number) => {
    const image = content?.image;
    const box = imageBoxRef.current;
    if (!image || !box) {
      return null;
    }
    const block = content!.imageBlocks[index];
    if (!block) {
      return null;
    }
    const rect = box.getBoundingClientRect();
    // 用"图片坐标 → 容器内像素"的换算，和 quadStyle 保持一致
    const xs = block.quad.map(([x]) => x);
    const ys = block.quad.map(([, y]) => y);
    const px = (Math.min(...xs) / image.width) * rect.width;
    const py = (Math.min(...ys) / image.height) * rect.height;
    const ph = ((Math.max(...ys) - Math.min(...ys)) / image.height) * rect.height;

    const width = Math.min(320, rect.width * 0.9);
    const left = Math.max(0, Math.min(px, rect.width - width));
    const estimated = 84;
    const above = py - estimated - 6;
    const top = above >= 0 ? above : py + ph + 6;

    return {
      left: `${left}px`,
      top: `${Math.max(0, top)}px`,
      text:
        blocks[index]?.status === "done"
          ? (blocks[index] as { text: string }).text
          : t("translateLoading"),
      original: block.text,
    };
  };

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

  // 气泡内容与位置（hover 时才算，避免每次渲染都量布局）
  const bubble = hovered === null ? null : bubbleFor(hovered);

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
              if (spokenWord !== null) {
                stopSpeaking();
              } else {
                startSpeaking(content?.text ?? "");
              }
            }}
            className={`ml-2 rounded border px-1.5 py-0.5 text-[11px] transition ${
              spokenWord !== null
                ? "border-[var(--hud-accent)] text-[var(--hud-accent)]"
                : "border-white/12 text-[var(--hud-muted)] hover:border-[var(--hud-accent)] hover:text-[var(--hud-accent)]"
            }`}
          >
            {spokenWord !== null ? t("speakStop") : t("speakStart")}
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
          <div
            ref={imageBoxRef}
            className="relative mx-auto w-fit"
            // 小截图放大到 2 倍：框按百分比定位，放大不会错位；
            // 否则 320px 的截图在 1200px 的窗口里，十几个框会挤成一团。
            // 用 width 而不是 maxWidth：父容器是 w-fit，maxWidth 撑不大它。
            style={{ width: content!.image!.width * 2, maxWidth: "100%" }}
          >
            <img src={imageSrc} alt="" className="w-full rounded-lg opacity-70" />

            {/* 原位格子：只放精简译文，不裁字也不盖住邻框（overflow 里 line-clamp 兜底） */}
            {content!.imageBlocks.map((block, index) => (
              <button
                type="button"
                key={index}
                onMouseEnter={() => setHovered(index)}
                onFocus={() => setHovered(index)}
                onMouseLeave={() => setHovered(null)}
                onBlur={() => setHovered(null)}
                className={`absolute overflow-hidden rounded border px-1 text-left transition ${
                  hovered === index
                    ? "z-10 border-[var(--hud-accent)] bg-[var(--hud-surface)]/95"
                    : "border-white/10 bg-[var(--hud-surface)]/80"
                }`}
                style={quadStyle(block)}
              >
                <span className="line-clamp-2 text-[11px] leading-[1.3]">
                  {blocks[index]?.status === "done"
                    ? (blocks[index] as { text: string }).text
                    : blocks[index]?.status === "loading"
                      ? t("translateLoading")
                      : "—"}
                </span>
              </button>
            ))}

            {/* 气泡：hover 时在本框上方或下方弹出**完整**译文，位置按剩余空间自动翻 */}
            {hovered !== null && bubble && (
              <div
                className="pointer-events-none absolute z-20 w-[min(320px,90%)] rounded-lg border border-[var(--hud-accent)]/60 bg-[var(--hud-surface)]/98 px-2 py-1.5 shadow-lg"
                style={{ left: bubble.left, top: bubble.top }}
              >
                <p className="text-[12px] leading-[1.5]">{bubble.text}</p>
                <p className="mt-1 line-clamp-2 text-[10px] leading-snug text-[var(--hud-muted)]">
                  {bubble.original}
                </p>
              </div>
            )}
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
                  // 小截图放大到 2 倍：框按百分比定位，放大不会错位；
            // 否则 320px 的截图在 1200px 的窗口里，十几个框会挤成一团。
            // 用 width 而不是 maxWidth：父容器是 w-fit，maxWidth 撑不大它。
            style={{ width: content!.image!.width * 2, maxWidth: "100%" }}
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
              (content?.blocks ?? []).map((block, index) => {
                const baseWord = (content?.blocks ?? [])
                  .slice(0, index)
                  .reduce(
                    (sum, item) =>
                      sum +
                      tokenize(item).filter((token) => token.kind === "word").length,
                    0,
                  );
                return (
                  <p
                    key={index}
                    className="mb-3 text-[13.5px] leading-[1.75] text-[var(--hud-text)]/90"
                  >
                    {renderClickable(block, true, baseWord)}
                  </p>
                );
              })
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
