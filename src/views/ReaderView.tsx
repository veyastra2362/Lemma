import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

import { t } from "../i18n";
import { ipc, isTauri, onReaderReload } from "../lib/ipc";
import type { Entry, ReaderBlock, ReaderContent, ReaderImage, SpeakResult } from "../types";
import { splitPosMarker, tokenize } from "../utils/tokens";

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

/**
 * 把文本切成句：中文按 `。！？；`、英文按 `. ! ? ;`，标点跟着前一句。
 *
 * 为什么不直接用 `core::reader::split_blocks`：那个是给"块级对齐"用的（几百字一块），
 * 朗读要的是**句子级**——一块一次合成太慢，失去流式的意义。
 */
function splitSentences(text: string): string[] {
  const ends = new Set(["。", "！", "？", "；", ".", "!", "?", ";", "\n"]);
  const sentences: string[] = [];
  let current = "";
  for (const ch of text) {
    if (ch === "\n") {
      if (current.trim()) {
        sentences.push(current.trim());
      }
      current = "";
      continue;
    }
    current += ch;
    if (ends.has(ch)) {
      if (current.trim()) {
        sentences.push(current.trim());
      }
      current = "";
    }
  }
  if (current.trim()) {
    sentences.push(current.trim());
  }
  return sentences;
}

/** 对照窗口自己的控件样式（与浮层同一套令牌）。 */
const READER_STYLES = `
  .rv-seg { display: inline-flex; border: 1px solid var(--hud-border); border-radius: var(--hud-r-ctl); overflow: hidden; }
  .rv-seg button {
    font: inherit; font-size: 11px; padding: 2px 9px; border: 0; background: transparent;
    color: var(--hud-muted); cursor: pointer;
  }
  .rv-seg button + button { border-left: 1px solid var(--hud-border); }
  .rv-seg button.is-on { background: var(--hud-accent-soft); color: var(--hud-accent); }
  .rv-btn {
    font: inherit; font-size: 11px; padding: 2px 8px;
    border: 1px solid var(--hud-border); border-radius: var(--hud-r-ctl);
    background: transparent; color: var(--hud-muted); cursor: pointer;
  }
  .rv-btn:hover { color: var(--hud-text); border-color: var(--hud-muted); }
  .rv-btn.is-on { border-color: var(--hud-accent); color: var(--hud-accent); }
  .rv-ico {
    font: inherit; font-size: 11px; width: 19px; height: 19px;
    display: inline-grid; place-items: center;
    border: 1px solid transparent; border-radius: var(--hud-r-ctl);
    background: transparent; color: var(--hud-faint); cursor: pointer;
  }
  .rv-ico:hover { color: var(--hud-text); border-color: var(--hud-border); }
  .rv-cols { display: grid; grid-template-columns: 1fr 1fr; min-height: 0; flex: 1; }
  .rv-col { min-height: 0; overflow-y: auto; padding: 14px 16px; }
  .rv-col-right { border-left: 1px solid var(--hud-border-soft); }
  /* 左图上的细框：只描边不填充，编号贴在框左上角 */
  .rv-box {
    position: absolute; border: 1px solid var(--hud-border); border-radius: 2px;
    background: transparent; padding: 0; cursor: pointer;
  }
  .rv-box:hover, .rv-box.is-hot { border-color: var(--hud-accent); }
  /* 编号贴在框**左侧外缘**：放在上方会在密集的截图里压到上一行 */
  .rv-box > .rv-num {
    position: absolute; left: 0; top: 50%; transform: translate(-100%, -50%);
    font-family: var(--hud-mono); font-size: 9.5px; line-height: 1.3;
    padding: 0 2px; border-radius: 2px 0 0 2px;
    background: var(--hud-surface); color: var(--hud-faint);
    border: 1px solid var(--hud-border-soft); border-right: 0;
  }
  .rv-box.is-hot > .rv-num { color: var(--hud-accent); border-color: var(--hud-accent); }
  /* 右栏逐块译文：编号在行首，hover 时左侧出现一条陶土色竖线 */
  .rv-speak {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    margin-left: 6px;
    padding: 1px 3px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--hud-faint);
    vertical-align: baseline;
    cursor: pointer;
    opacity: 0.55;
    transition: opacity var(--hud-t) ease-out, color var(--hud-t) ease-out;
  }
  .rv-speak:hover { opacity: 1; color: var(--hud-accent); }
  .rv-block {
    display: flex; gap: 8px; align-items: flex-start;
    padding: 2px 0; margin-bottom: 10px;
    border-left: 1px solid transparent;
    cursor: default;
  }
  .rv-block > .rv-num {
    position: static; transform: none; flex: none;
    font-size: 10px; padding: 0 3px; border-radius: 2px;
    border: 1px solid var(--hud-border-soft); background: transparent;
  }
  .rv-block.is-hot { border-left-color: var(--hud-accent); padding-left: 6px; }
  .rv-block.is-hot > .rv-num { color: var(--hud-accent); border-color: var(--hud-accent); }
`;

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

/** 每句末尾的朗读按钮（原文侧与译文侧共用）。 */
function SpeakLine({
  onSpeak,
  label,
}: {
  onSpeak: () => void;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        onSpeak();
      }}
      title={label}
      aria-label={label}
      className="rv-speak"
    >
      <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true" fill="none">
        <path d="M2 4.5h1.8L6 2.6v6.8L3.8 7.5H2z" stroke="currentColor" strokeWidth="1.1" strokeLinejoin="round" />
        <path d="M8 4.2c.6.5.6 3.1 0 3.6" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
      </svg>
    </button>
  );
}
export function ReaderView() {
  const [content, setContent] = useState<ReaderContent | null>(null);
  const [blocks, setBlocks] = useState<BlockState[]>([]);
  /**
   * 底部词条面板的内容（**只有一条**）。
   *
   * 原来这里是个下探栈 + 面包屑，但双栏底部只是一块"随手查一下"的附注面板：
   * 再点一个词就该把内容换掉，而不是压一层新层级。层级只在 HUD 的卡片栈里有意义。
   */
  const [lookup, setLookup] = useState<Drill | null>(null);
  const [mode, setMode] = useState<ViewMode>("side");
  const [hovered, setHovered] = useState<number | null>(null);
  const [speechReady, setSpeechReady] = useState(false);
  /** 两个朗读按钮是否显示（设置里可关）。 */
  const [speakOriginal, setSpeakOriginal] = useState(true);
  const [speakTranslation, setSpeakTranslation] = useState(true);
  /** 是否显示快捷键提示（设置里可关）。 */
  const [showHints, setShowHints] = useState(true);
  /** 跟读：正在朗读的词（全局序号），null 表示没在朗读。 */
  const [spokenWord, setSpokenWord] = useState<number | null>(null);
  /** 是否正在朗读（含"读译文"，那种情况不逐词高亮）。 */
  const [speaking, setSpeaking] = useState(false);
  /** 正在读的是原文还是译文。 */
  const [speakingSource, setSpeakingSource] = useState<"original" | "translation" | null>(null);
  const speakTimer = useRef<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const imageBoxRef = useRef<HTMLDivElement>(null);
  /** 当前文本的快照，供冒烟事件读取（事件回调里拿不到最新的 state 闭包）。 */
  const contentRef = useRef<string>("");
  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const syncing = useRef(false);
  const loadSeq = useRef(0);
  const scrollMemory = useRef<{ text: string; left: number; right: number } | null>(null);

  const isImage = Boolean(content?.image && content.imageBlocks.length > 0);
  const sources = isImage
    ? (content?.imageBlocks ?? []).map((block: ReaderBlock) => block.text)
    : (content?.blocks ?? []);

  const stopSpeaking = useCallback(() => {
    if (speakTimer.current !== null) {
      window.clearTimeout(speakTimer.current);
      speakTimer.current = null;
    }
    setSpokenWord(null);
    setSpeaking(false);
    setSpeakingSource(null);
    void ipc.stopSpeaking();
  }, []);

  /**
   * 跟读高亮。
   *
   * `PlaySound` 不汇报播放位置，所以这里按**字符权重**把总时长线性摊到每个词上：
   * 长词占的时间多、短词少。顺序与合成时的音素顺序一致（同一段文本、同一个分词）。
   * 这是估算而不是真同步，足够让视线跟着走；精确对齐需要换能报位置的播放后端。
   */
  /**
   * 流式朗读：**分句合成 + 预取**。
   *
   * 为什么：整段一次合成时，首句要等整段算完（实测长段落 4 s+）。
   * 现在先合成第一句就开播，播的同时合成第二句——首句延迟从"整段"降到"一句"。
   *
   * 合成与播放要分开：`synthesizeSpeech` 只出音频与时长，`playAudio` 才出声，
   * 这样"播放当前句"和"合成下一句"能重叠。
   */
  const startSpeaking = useCallback(
    (text: string, source: "original" | "translation" = "original") => {
      const highlight = source === "original";
      setSpeaking(true);
      setSpeakingSource(source);
      if (speakTimer.current !== null) {
        window.clearTimeout(speakTimer.current);
        speakTimer.current = null;
      }
      setSpokenWord(null);

      const sentences = splitSentences(text);
      if (sentences.length === 0) {
        return;
      }

      let index = 0;
      let stopped = false;
      let prefetched: Promise<SpeakResult> | null = null;
      let wordOffset = 0;

      const fail = (reason: unknown) => {
        stopped = true;
        setSpokenWord(null);
        setSpeaking(false);
        setSpeakingSource(null);
        void ipc.logFrontend("error", "reader speak failed: " + String(reason));
      };

      const highlightSentence = (sentence: string, durationSecs: number) => {
        const sizes = tokenize(sentence)
          .filter((token) => token.kind === "word")
          .map((token) => token.value.length);
        const total = sizes.reduce((sum, size) => sum + size, 0) || 1;
        let local = 0;
        const step = () => {
          if (stopped || local >= sizes.length) {
            if (!stopped) {
              wordOffset += sizes.length;
            }
            return;
          }
          setSpokenWord(wordOffset + local);
          const share = ((sizes[local] ?? 1) / total) * durationSecs * 1000;
          local += 1;
          speakTimer.current = window.setTimeout(step, Math.max(share, 60));
        };
        step();
      };

      const playNext = () => {
        if (stopped) {
          return;
        }
        if (index >= sentences.length) {
          setSpokenWord(null);
          setSpeaking(false);
          setSpeakingSource(null);
          speakTimer.current = null;
          return;
        }
        const current = index;
        const task = prefetched ?? ipc.synthesizeSpeech(sentences[current]!);
        prefetched = null;

        void task
          .then((result) => {
            if (stopped) {
              return;
            }
            void ipc.playAudio(result.path).catch(fail);
            // 只有读原文时逐词高亮；读译文时译文那一栏不是逐词渲染的，高亮无从落点
            if (highlight) {
              highlightSentence(sentences[current]!, result.durationSecs);
            }

            // 一边播一边把下一句合成好
            if (current + 1 < sentences.length) {
              prefetched = ipc.synthesizeSpeech(sentences[current + 1]!);
              // 提前挂上错误处理，避免未捕获的 rejection
              void prefetched.catch(fail);
            }

            index = current + 1;
            speakTimer.current = window.setTimeout(
              playNext,
              Math.max(result.durationSecs * 1000, 80),
            );
          })
          .catch(fail);
      };

      playNext();
      void ipc.logFrontend("info", `speak stream sentences=${sentences.length}`);
    },
    [],
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

    setLookup(null);
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
        contentRef.current = payload.text;

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
    void ipc
      .getSettings()
      .then((value) => {
        setSpeakOriginal(value.speakOriginal);
        setSpeakTranslation(value.speakTranslation);
        setShowHints(value.showHints);
      })
      .catch(() => undefined);
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
      if (lookup) {
        setLookup(null);
        return;
      }
      void ipc.closeReader();
    };
    window.addEventListener("keydown", onKeyDown);

    let disposed = false;
    let unlisten: (() => void) | undefined;
    // 冒烟：Rust 侧请求"立刻朗读一次"，用于验证流式流水线（正文见 docs）
    let unlistenSpeak: (() => void) | undefined;
    void listen("reader://autospeak", () => {
      startSpeaking(contentRef.current ?? "");
    })
      .then((fn) => {
        unlistenSpeak = fn;
      })
      .catch(() => undefined);

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
      unlistenSpeak?.();
      unlisten?.();
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [lookup, load]);

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
    setLookup({ word: term, entry: null });
    void ipc
      .lookupWord(term)
      .then((entry) => {
        // 只更新"还是这个词"的情况：用户可能已经点了别的词
        setLookup((prev) => (prev && prev.word === term ? { ...prev, entry } : prev));
      })
      .catch(() => undefined);
  };

  const current = lookup;

  /**
   * 渲染可点的词。
   *
   * `allowCjk = false` 时中文不参与点击：用户要求「点击范围先最小」——
   * 中文释义只在**释义/译文**一侧可点，原文那一侧保持纯净，避免阅读时误触。
   */
  const renderClickable = (text: string, baseWord = -1, allowCjk = false) => {
    // baseWord >= 0 时把该段之前的累计词数加上，得到"全文第几个词"，用于跟读高亮
    let seen = 0;
    return (
      <>
        {tokenize(text).map((token, index) => {
          const clickable =
            token.kind === "word" && (allowCjk || !/[\u3400-\u9fff]/.test(token.value));
          const wordIndex = clickable ? baseWord + seen++ : -1;
          const isSpoken = baseWord >= 0 && wordIndex === spokenWord;
          if (!clickable) {
            return <span key={`${index}-sep`}>{token.value}</span>;
          }
          return (
            <button
              type="button"
              key={`${index}-${token.value}`}
              onClick={() => pushWord(token.value)}
              className={`border-b border-dashed pb-px border-[var(--hud-border)] hover:border-[var(--hud-muted)] hover:text-[var(--hud-text)] ${
                isSpoken ? "border-solid border-[var(--hud-accent)] bg-[var(--hud-accent-soft)]" : ""
              }`}
            >
              {token.value}
            </button>
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
        <p key="loading" className="fade-in text-[12px] text-[var(--hud-muted)]">
          {t("translateLoading")}
        </p>
      );
    }
    if (block.status === "error") {
      return <p className="text-[11px] leading-relaxed text-red-300/80">{block.error}</p>;
    }
    return (
      <p
        key="done"
        className={`fade-in ${compact ? "text-[12px] leading-[1.6]" : "text-[13.5px] leading-[1.75]"}`}
      >
        {/* 译文这一侧允许点中文（查汉语词典）；原文那一侧不允许，避免误触 */}
        {renderClickable(block.text, -1, true)}
        <SpeakLine
          onSpeak={() => speakAs("translation", block.text)}
          label="朗读这一句译文"
        />
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

  /** 译文全文（按块拼起来），供「朗读译文」使用。 */
  const translationText = blocks
    .map((block) => (block.status === "done" ? (block as { text: string }).text : ""))
    .filter((text) => text.trim().length > 0)
    .join(" ");

  const speakAs = (source: "original" | "translation", text: string) => {
    startSpeaking(text, source);
  };

  return (
    <div className="lm-panel flex flex-col text-[var(--hud-text)]">
      <style>{READER_STYLES}</style>

      {/* 只有标题文字可拖拽：整块 header 挂 drag-region 会把按钮的点击吞掉（实测） */}
      <header className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--hud-border-soft)] px-3 py-1.5">
        <span data-tauri-drag-region className="text-[12px]">
          {t("readerTitle")}
        </span>
        {content?.app && (
          <span className="text-[11px] text-[var(--hud-faint)]">{content.app}</span>
        )}
        <span className="font-mono text-[10.5px] tabular-nums text-[var(--hud-faint)]">
          {sources.length} {t("readerBlocks")}
        </span>

        {isImage && (
          <div className="rv-seg ml-1">
            {(["side", "overlay"] as const).map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => setMode(option)}
                className={mode === option ? "is-on" : ""}
              >
                {option === "side" ? t("readerModeSide") : t("readerModeOverlay")}
              </button>
            ))}
          </div>
        )}

        <div className="ml-auto flex items-center gap-1.5">
          {speechReady && speakOriginal && (content?.text ?? "").trim().length > 0 && (
            <button
              type="button"
              onClick={() => {
                if (speaking) {
                  stopSpeaking();
                } else {
                  startSpeaking(content?.text ?? "");
                }
              }}
              className={`rv-btn ${speakingSource === "original" && speaking ? "is-on" : ""}`}
            >
              {speakingSource === "original" && speaking ? t("speakStop") : t("speakOriginal")}
            </button>
          )}
          {speechReady && speakTranslation && translationText.trim().length > 0 && (
            <button
              type="button"
              onClick={() => {
                if (speakingSource === "translation" && speaking) {
                  stopSpeaking();
                } else {
                  speakAs("translation", translationText);
                }
              }}
              className={`rv-btn ${speakingSource === "translation" && speaking ? "is-on" : ""}`}
            >
              {speakingSource === "translation" && speaking
                ? t("speakStop")
                : t("speakTranslation")}
            </button>
          )}
          <button
            type="button"
            onClick={() => void ipc.closeReader()}
            aria-label={t("settingsClose")}
            className="rv-ico"
          >
            ✕
          </button>
        </div>
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
            <img src={imageSrc} alt="" className="w-full rounded-[var(--hud-r-ctl)] opacity-70" />

            {/* 原位格子：只放精简译文，不裁字也不盖住邻框（overflow 里 line-clamp 兜底） */}
            {content!.imageBlocks.map((block, index) => (
              <button
                type="button"
                key={index}
                onMouseEnter={() => setHovered(index)}
                onFocus={() => setHovered(index)}
                onMouseLeave={() => setHovered(null)}
                onBlur={() => setHovered(null)}
                className={`absolute overflow-hidden rounded-[2px] border px-1 text-left ${
                  hovered === index
                    ? "z-10 border-[var(--hud-accent)] bg-[var(--hud-surface)]"
                    : "border-[var(--hud-border)] bg-[var(--hud-surface)]"
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
                className="pointer-events-none absolute z-20 w-[min(320px,90%)] rounded-[var(--hud-r-ctl)] border border-[var(--hud-accent)] bg-[var(--hud-surface)] px-2 py-1.5"
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

      {/* ---------- 并排模式：左截图带细框，右编号译文 ---------- */}
      {(!isImage || mode === "side") && (
        <div className="rv-cols">
          <div
            ref={leftRef}
            onScroll={() => syncScroll("left")}
            className="rv-col"
          >
            {isImage && imageSrc ? (
              <div
                className="relative mx-auto w-fit pl-[14px]"
                // 小截图放大到 2 倍：框按百分比定位，放大不会错位
                style={{ width: content!.image!.width * 2, maxWidth: "100%" }}
              >
                <img src={imageSrc} alt="" className="w-full rounded-[var(--hud-r-ctl)]" />
                {content!.imageBlocks.map((block, index) => (
                  <button
                    type="button"
                    key={index}
                    onMouseEnter={() => setHovered(index)}
                    onFocus={() => setHovered(index)}
                    onMouseLeave={() => setHovered(null)}
                    onBlur={() => setHovered(null)}
                    className={`rv-box ${hovered === index ? "is-hot" : ""}`}
                    style={quadStyle(block)}
                  >
                    <span className="rv-num">{index + 1}</span>
                  </button>
                ))}
              </div>
            ) : (
              (content?.blocks ?? []).map((block, index) => {
                const baseWord = (content?.blocks ?? [])
                  .slice(0, index)
                  .reduce(
                    (sum, item) =>
                      sum + tokenize(item).filter((token) => token.kind === "word").length,
                    0,
                  );
                return (
                  <p key={index} className="mb-3 text-[13.5px] leading-[1.75]">
                    {renderClickable(block, baseWord)}
                    <SpeakLine
                      onSpeak={() => speakAs("original", block)}
                      label="朗读这一句"
                    />
                  </p>
                );
              })
            )}
          </div>

          <div
            ref={rightRef}
            onScroll={() => syncScroll("right")}
            className="rv-col rv-col-right"
          >
            {sources.map((_, index) => (
              <div
                key={index}
                onMouseEnter={() => setHovered(index)}
                onMouseLeave={() => setHovered(null)}
                className={`rv-block ${hovered === index ? "is-hot" : ""}`}
              >
                <span className="rv-num">{index + 1}</span>
                <div className="min-w-0">{renderTranslation(index, false)}</div>
              </div>
            ))}
          </div>
        </div>
      )}
      {current && (
        <footer className="fade-in shrink-0 border-t border-[var(--hud-border-soft)] bg-[var(--hud-raised)] px-4 py-2">
          <div className="flex items-center gap-2">
            <span className="text-[12px] text-[var(--hud-text)]">{current.word}</span>
            {current.entry?.phonetic && (
              <span className="font-mono text-[11px] text-[var(--hud-muted)]">
                /{current.entry.phonetic}/
              </span>
            )}
            {showHints && (
              <span className="ml-auto text-[11px] text-[var(--hud-muted)] opacity-70">
                {t("readerEscHint")}
              </span>
            )}
            <button
              type="button"
              onClick={() => setLookup(null)}
              className={`${showHints ? "ml-2" : "ml-auto"} text-[11px] text-[var(--hud-muted)] hover:text-[var(--hud-text)]`}
            >
              {t("readerLookupClose")}
            </button>
          </div>

          {current.entry ? (
            <div className="mt-0.5 max-h-32 space-y-0.5 overflow-y-auto">
              {current.entry.translation
                .split("\n")
                .filter((line) => line.trim().length > 0)
                .slice(0, 5)
                .map((line, index) => {
                  // 词性标记（`n.` / `[计]`）不是可查的词
                  const [marker, body] = splitPosMarker(line);
                  return (
                    <p key={index} className="text-[12px] leading-relaxed">
                      {marker && <span>{marker}</span>}
                      {renderClickable(body)}
                    </p>
                  );
                })}
            </div>
          ) : (
            <p className="mt-0.5 text-[11px] text-[var(--hud-muted)]">{t("cardNotInDict")}</p>
          )}
        </footer>
      )}
    </div>
  );
}
