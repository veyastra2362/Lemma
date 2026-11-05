import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import { ipc, isTauri } from "./lib/ipc";
import { parseRoute } from "./utils/route";
import { ReaderView } from "./views/ReaderView";
import { RegionSelectView } from "./views/RegionSelectView";
import { SettingsView } from "./views/SettingsView";
import { ShellView } from "./views/ShellView";

export default function App() {
  // 三个窗口共用同一份产物，靠 hash 决定渲染哪个视图；窗口之间不会互相导航。
  const [route] = useState(() => parseRoute(window.location.hash));

  useEffect(() => {
    if (isTauri()) {
      void ipc
        .frontendReady(`${route} ${window.location.hash || "(空)"}`)
        .catch(() => {
          // 就绪回报失败不影响功能，Rust 侧有预热超时兜底。
        });
    }
  }, [route]);

  // 强调色是**全局**令牌：四个窗口各自是独立 webview，所以每个窗口都要自己应用一次，
  // 并在设置变更时跟随（Rust 侧 update_settings 会广播 settings://changed）。
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    const apply = (accent: string) => {
      document.documentElement.dataset.accent = accent === "olive" ? "olive" : "clay";
    };
    void ipc
      .getSettings()
      .then((value) => apply(value.accent))
      .catch(() => apply("clay"));
    let unlisten: (() => void) | undefined;
    void listen<{ accent?: string }>("settings://changed", (event) => {
      apply(event.payload?.accent ?? "clay");
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => undefined);
    return () => unlisten?.();
  }, []);

  switch (route) {
    case "settings":
      return <SettingsView />;
    case "ocr-region":
      return <RegionSelectView />;
    case "reader":
      return <ReaderView />;
    case "shell":
    default:
      return <ShellView />;
  }
}
