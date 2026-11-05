/**
 * 把释义文本切成「可点的词」与「不可点的分隔」。
 *
 * 规则：
 * - 英文按词切（字母开头，允许词中撇号与连字符，如 `don't` / `e-mail`）；
 * - **CJK 连续串整体作为一个词**——UIA 的 WordUnit 不分中文，这里也不假装能分，
 *   点击后交给词库判断收录与否。真正的分词等接入中文词典再做（M2.1）；
 * - 其余（数字、标点、空白）都算分隔，不响应点击。
 */
export type TokenKind = "word" | "sep";

export interface Token {
  kind: TokenKind;
  value: string;
}

const WORD_PATTERN = /[A-Za-z][A-Za-z'’-]*|\p{Script=Han}+/gu;

export function tokenize(text: string): Token[] {
  const tokens: Token[] = [];
  let last = 0;

  for (const match of text.matchAll(WORD_PATTERN)) {
    const index = match.index ?? 0;
    if (index > last) {
      tokens.push({ kind: "sep", value: text.slice(last, index) });
    }
    tokens.push({ kind: "word", value: match[0] });
    last = index + match[0].length;
  }

  if (last < text.length) {
    tokens.push({ kind: "sep", value: text.slice(last) });
  }

  return tokens;
}

/** 术语规范化，和后端 `dict::normalize` 保持同一套规则（用于判断"是否已在该卡片里"）。 */
export function normalizeTerm(word: string): string {
  return word.trim().toLowerCase();
}

/**
 * 释义行开头的**词性 / 领域标记**不是可以查的词，要拆出来当纯文本。
 *
 * ECDICT 的释义形如 `n. 表征, 记号, 代词`、`[计] 记账`、`a. 象征的`。
 * 不拆的话 `n`、`a` 会被 tokenize 当成单词，变成可点链接——点了去查 "n"，
 * 毫无意义（用户实测指出过）。
 *
 * 返回 `[标记, 正文]`；没有标记时标记为空串。
 */
export function splitPosMarker(line: string): [string, string] {
  const match = line.match(/^(\[[^\]]{1,8}\]|[A-Za-z]{1,4}\.)[ \t]*/);
  if (!match) {
    return ["", line];
  }
  return [match[0], line.slice(match[0].length)];
}
