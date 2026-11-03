import { describe, expect, it } from "vitest";

import { formatHotkey } from "./hotkey";

function key(
  code: string,
  mods: Partial<Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey">> = {},
) {
  return {
    code,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...mods,
  };
}

describe("formatHotkey", () => {
  it("把常见组合格式化成 Tauri 能解析的写法", () => {
    expect(formatHotkey(key("KeyQ", { altKey: true }))).toBe("Alt+Q");
    expect(formatHotkey(key("KeyD", { ctrlKey: true, shiftKey: true }))).toBe(
      "Ctrl+Shift+D",
    );
    expect(formatHotkey(key("F2", { altKey: true }))).toBe("Alt+F2");
  });

  it("修饰键顺序固定，避免同一组合出现多种写法", () => {
    expect(formatHotkey(key("KeyX", { shiftKey: true, altKey: true, ctrlKey: true }))).toBe(
      "Ctrl+Alt+Shift+X",
    );
  });

  it("拒绝没有强修饰键的组合", () => {
    expect(formatHotkey(key("KeyQ"))).toBeNull();
    expect(formatHotkey(key("KeyQ", { shiftKey: true }))).toBeNull();
  });

  it("拒绝不可作为主键的按键", () => {
    expect(formatHotkey(key("ControlLeft", { ctrlKey: true }))).toBeNull();
    expect(formatHotkey(key("ShiftLeft", { ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(formatHotkey(key("Tab", { ctrlKey: true }))).toBeNull();
  });
});
