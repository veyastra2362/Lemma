import { create } from "zustand";

import type { Capture, Entry } from "../types";

/** 一张卡片的翻译状态。 */
export type TranslationState =
  | { status: "idle" }
  | { status: "loading" }
  | {
      status: "done";
      text: string;
      provider: string;
      elapsedMs: number;
      cached: boolean;
    }
  | { status: "error"; error: string };

/** 卡片栈里的一张卡片。 */
export interface Card {
  id: number;
  /** 查的词（词卡片）或原文（句卡片）。 */
  word: string;
  /** 句卡片的原文；词卡片为 null。 */
  original: string | null;
  /** 词库结果；未收录或句卡片时为 null。 */
  entry: Entry | null;
  /** 句卡片的译文状态；词卡片恒为 idle。 */
  translation: TranslationState;
  /** 产生这张卡片的来源文案（`chrome.exe · 选区`），根卡片才显示。 */
  source: string | null;
  /** 取词耗时，仅根卡片显示。 */
  elapsedMs: number | null;
}

const MAX_DEPTH = 24;

interface DeckState {
  cards: Card[];
  nextId: number;
  /** 用一次新的取词结果重置整个栈（每次呼出 HUD 都会调）。 */
  reset: (capture: Capture) => void;
  /** 下探一层。返回 false 表示这个词已经在栈里（防自环）。 */
  push: (word: string, entry: Entry | null) => boolean;
  /** 更新某张卡片的翻译状态。 */
  setTranslation: (id: number, state: TranslationState) => void;
  /** 退一层。返回 false 表示已经是最底层（调用方应改为关闭 HUD）。 */
  pop: () => boolean;
  /** 跳回某一层，中间层丢掉。 */
  jumpTo: (index: number) => void;
  clear: () => void;
}

export const useDeckStore = create<DeckState>((set, get) => ({
  cards: [],
  nextId: 1,

  reset: (capture) => {
    const raw = (capture.text ?? "").trim();
    const isWordCard = !capture.needsTranslation;
    const word = isWordCard ? (capture.entry?.word || raw).trim() : raw;

    const card: Card = {
      id: 0,
      word,
      original: isWordCard ? null : raw,
      entry: capture.entry ?? null,
      translation: { status: "idle" },
      source: `${capture.app || "?"} · ${capture.tier ?? "-"}`,
      elapsedMs: capture.elapsedMs,
    };
    set({ cards: word ? [card] : [], nextId: 1 });
  },

  push: (word, entry) => {
    const term = word.trim();
    if (!term) {
      return false;
    }
    const { cards, nextId } = get();
    // 同一个栈里重复查同一个词没有意义，直接判为失败，让 UI 什么都不做。
    if (cards.some((card) => card.word.toLowerCase() === term.toLowerCase())) {
      return false;
    }
    if (cards.length >= MAX_DEPTH) {
      return false;
    }
    set({
      cards: [
        ...cards,
        {
          id: nextId,
          word: term,
          original: null,
          entry,
          translation: { status: "idle" },
          source: null,
          elapsedMs: null,
        },
      ],
      nextId: nextId + 1,
    });
    return true;
  },

  setTranslation: (id, state) => {
    const { cards } = get();
    if (!cards.some((card) => card.id === id)) {
      return;
    }
    set({
      cards: cards.map((card) =>
        card.id === id ? { ...card, translation: state } : card,
      ),
    });
  },

  pop: () => {
    const { cards } = get();
    if (cards.length <= 1) {
      return false;
    }
    set({ cards: cards.slice(0, -1) });
    return true;
  },

  jumpTo: (index) => {
    const { cards } = get();
    if (index < 0 || index >= cards.length) {
      return;
    }
    set({ cards: cards.slice(0, index + 1) });
  },

  clear: () => set({ cards: [], nextId: 1 }),
}));
