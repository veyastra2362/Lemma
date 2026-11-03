import { useEffect } from "react";

import { ipc, isTauri, onSettingsChanged } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";

/**
 * 拉一次运行期设置，并订阅后续变更。
 *
 * 为什么需要订阅：设置窗口和 HUD 是两个独立的 webview，各自持有独立的前端状态。
 * 在设置窗口里改了"卡片显示哪些区块"，HUD 不会自动知道——必须由 Rust 广播。
 */
export function useRuntimeInfo(): void {
  const setSettings = useAppStore((state) => state.setSettings);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let disposed = false;

    void ipc
      .getSettings()
      .then((settings) => {
        if (!disposed) {
          setSettings(settings);
        }
      })
      .catch(() => undefined);

    void onSettingsChanged((settings) => {
      if (!disposed) {
        setSettings(settings);
      }
    }).catch(() => undefined);

    return () => {
      disposed = true;
    };
  }, [setSettings]);
}
