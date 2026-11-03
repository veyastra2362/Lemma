import { describe, expect, it } from "vitest";

import { normalizeTerm, tokenize } from "./tokens";

const words = (text: string) =>
  tokenize(text)
    .filter((token) => token.kind === "word")
    .map((token) => token.value);

describe("tokenize", () => {
  it("拆出英文词", () => {
    expect(words("n. 门槛, 入口")).toEqual(["n", "门槛", "入口"]);
    expect(words("The threshold value matters.")).toEqual([
      "The",
      "threshold",
      "value",
      "matters",
    ]);
  });

  it("保留词内的撇号与连字符", () => {
    expect(words("don't be e-mail")).toEqual(["don't", "be", "e-mail"]);
  });

  it("CJK 连续串整体算一个词（不做假分词）", () => {
    const tokens = tokenize("门槛, 入口");
    const cjk = tokens.filter((token) => token.value === "门槛");
    expect(cjk).toHaveLength(1);
    expect(cjk[0]?.kind).toBe("word");
  });

  it("标点与空白不产生可点词", () => {
    expect(words("。，！？ …—")).toEqual([]);
    expect(words("   ")).toEqual([]);
  });

  it("保留原文顺序与完整内容（可无损还原）", () => {
    const source = "n. 门槛, 入口 threshold's";
    expect(
      tokenize(source)
        .map((token) => token.value)
        .join(""),
    ).toBe(source);
  });
});

describe("normalizeTerm", () => {
  it("去空白并转小写", () => {
    expect(normalizeTerm("  Threshold ")).toBe("threshold");
  });
});
