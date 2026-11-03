/**
 * 把一次按键事件格式化成 Tauri 能解析的热键字符串。
 *
 * 规则：
 * - 必须含 Ctrl / Alt / Super 之一（只有 Shift 的组合会和正常输入打架）；
 * - 主键只接受字母、数字、F1–F12、方向键、编辑键与常见标点，避免录进 "Control" 自身；
 * - 字母键按 `KeyboardEvent.code` 取（`KeyQ` → `Q`），这样不受输入法与键盘布局影响。
 */
const ALLOWED_CODE =
  /^(Key[A-Z]|Digit[0-9]|F(?:[1-9]|1[0-2])|Arrow(?:Up|Down|Left|Right)|Space|Enter|Home|End|PageUp|PageDown|Insert|Delete|Backquote|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash)$/;

type KeyLike = {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
};

export function formatHotkey(event: KeyLike): string | null {
  if (!ALLOWED_CODE.test(event.code)) {
    return null;
  }
  // 只按 Shift 不算热键：Shift+Q 就是打个大写 Q。
  if (!event.ctrlKey && !event.altKey && !event.metaKey) {
    return null;
  }

  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Super");

  const key = event.code.startsWith("Key") ? event.code.slice(3) : event.code;
  parts.push(key);
  return parts.join("+");
}
