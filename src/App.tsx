import { useState } from "react";

import { parseRoute } from "./utils/route";
import { RegionSelectView } from "./views/RegionSelectView";
import { SettingsView } from "./views/SettingsView";
import { ShellView } from "./views/ShellView";

export default function App() {
  // 三个窗口共用同一份产物，靠 hash 决定渲染哪个视图；窗口之间不会互相导航。
  const [route] = useState(() => parseRoute(window.location.hash));

  switch (route) {
    case "settings":
      return <SettingsView />;
    case "ocr-region":
      return <RegionSelectView />;
    case "shell":
    default:
      return <ShellView />;
  }
}
