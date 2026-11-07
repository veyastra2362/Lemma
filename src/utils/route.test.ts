import { describe, expect, it } from "vitest";

import { parseRoute } from "./route";

describe("parseRoute", () => {
  it("识别三个已知窗口路由", () => {
    expect(parseRoute("#/shell")).toBe("shell");
    expect(parseRoute("#/settings")).toBe("settings");
    expect(parseRoute("#/ocr-region")).toBe("ocr-region");
  });

  it("容忍没有斜杠、多余斜杠与大小写", () => {
    expect(parseRoute("#settings")).toBe("settings");
    expect(parseRoute("#/settings/")).toBe("settings");
    expect(parseRoute("#/SETTINGS")).toBe("settings");
  });

  it("空 hash 与未知路由回落到 HUD，避免白屏", () => {
    expect(parseRoute("")).toBe("shell");
    expect(parseRoute("#")).toBe("shell");
    expect(parseRoute("#/nope")).toBe("shell");
  });
});
