import { beforeEach, describe, expect, it } from "vitest";

import { shouldTranslate, useDeckStore, type Card } from "./useDeckStore";
import type { Capture } from "../types";

function capture(overrides: Partial<Capture> = {}): Capture {
  return {
    text: "Grok is an AI assistant built by SpaceXAI.",
    sentence: "",
    paragraph: "",
    tier: "clipboard",
    entry: null,
    needsTranslation: true,
    app: "chrome.exe",
    elapsedMs: 19,
    error: null,
    ...overrides,
  };
}

function root(): Card | undefined {
  return useDeckStore.getState().cards[0];
}

describe("useDeckStore", () => {
  beforeEach(() => {
    useDeckStore.getState().clear();
  });

  it("句子取词会建一张句卡片，并标记为待翻译", () => {
    useDeckStore.getState().reset(capture());
    const card = root();
    expect(card?.original).toBe("Grok is an AI assistant built by SpaceXAI.");
    expect(card?.entry).toBeNull();
    expect(shouldTranslate(card)).toBe(true);
  });

  it("单词取词建词卡片，不触发翻译", () => {
    useDeckStore.getState().reset(
      capture({
        text: "assistant",
        needsTranslation: false,
        entry: {
          word: "assistant",
          phonetic: "'sɪstənt",
          translation: "n. 助手",
          definition: "",
          pos: "",
          tag: "",
          exchange: "",
          bnc: 0,
          frq: 0,
        },
      }),
    );
    const card = root();
    expect(card?.word).toBe("assistant");
    expect(card?.entry?.translation).toBe("n. 助手");
    expect(shouldTranslate(card)).toBe(false);
  });

  it("翻译进行中或已完成时不会重复发起", () => {
    useDeckStore.getState().reset(capture());
    const id = root()!.id;

    useDeckStore.getState().setTranslation(id, { status: "loading" });
    expect(shouldTranslate(root())).toBe(false);

    useDeckStore.getState().setTranslation(id, {
      status: "done",
      text: "译文",
      provider: "google",
      elapsedMs: 900,
      cached: false,
    });
    expect(shouldTranslate(root())).toBe(false);
    expect(root()?.translation).toMatchObject({ status: "done", text: "译文" });
  });

  /**
   * 回归：早期用卡片 id 去重，而 reset 后根卡片 id 恒为 0，
   * 导致「只有第一次会翻译」。每次 reset 都必须重新可翻译。
   */
  it("连续多次呼出，每次都重新发起翻译", () => {
    for (let round = 0; round < 3; round += 1) {
      useDeckStore.getState().reset(capture());
      expect(shouldTranslate(root())).toBe(true);

      const id = root()!.id;
      useDeckStore.getState().setTranslation(id, { status: "loading" });
      expect(shouldTranslate(root())).toBe(false);
    }
  });

  it("同样的词不会重复下探", () => {
    useDeckStore.getState().reset(capture());
    expect(useDeckStore.getState().push("assistant", null)).toBe(true);
    expect(useDeckStore.getState().push("assistant", null)).toBe(false);
    expect(useDeckStore.getState().cards).toHaveLength(2);
  });

  it("退层到底后返回 false，让调用方改为关窗", () => {
    useDeckStore.getState().reset(capture());
    expect(useDeckStore.getState().pop()).toBe(false);
    useDeckStore.getState().push("assistant", null);
    expect(useDeckStore.getState().pop()).toBe(true);
    expect(useDeckStore.getState().pop()).toBe(false);
  });

  it("面包屑跳回会丢掉更深层", () => {
    useDeckStore.getState().reset(capture());
    useDeckStore.getState().push("assistant", null);
    useDeckStore.getState().push("built", null);
    expect(useDeckStore.getState().cards).toHaveLength(3);

    useDeckStore.getState().jumpTo(0);
    expect(useDeckStore.getState().cards).toHaveLength(1);
  });
});
