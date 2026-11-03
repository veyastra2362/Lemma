import { create } from "zustand";

import type { Capture, Entry } from "../types";

/** 卡片栈里的一张卡片。 */
export interface Card {
  id: number;
  /** 这张卡片查的词。 */
  word: string;
  /** 词库结果；未收录时为 null。 */
  entry: Entry | null;
  /** 产生这张卡片的来源文案（`chrome.exe` / `光标词`），根卡片才显示。 */
  source: string | null;
  /** 取词/查询耗时，仅根卡片显示。 */
  elapsedMs: number | null;
}

const MAX_DEPTH = 24;

interface DeckState {
  cards: Card[];
  nextId: number;
  /** 用一次新的取词结果重置整个栈（每次呼出 HUD 都会调）。 */
  reset: (capture: Capture) => void;
  /** 下探一层。返回 false 表示这个词已经在栈里（防止自环）。 */
  push: (word: string, entry: Entry | null) => boolean;
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
    const word = (capture.entry?.word || capture.text || "").trim();
    const card: Card = {
      id: 0,
      word,
      entry: capture.entry ?? null,
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
    // 同一层里重复查同一个词没有意义，直接判为失败，让 UI 什么都不做。
    if (cards.some((card) => card.word.toLowerCase() === term.toLowerCase())) {
      return false;
    }
    if (cards.length >= MAX_DEPTH) {
      return false;
    }
    set({
      cards: [...cards, { id: nextId, word: term, entry, source: null, elapsedMs: null }],
      nextId: nextId + 1,
    });
    return true;
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
