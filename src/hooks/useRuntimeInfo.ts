import { useEffect } from "react";

import { ipc, isTauri } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";

/**
 * 拉一次运行期设置（触发方式与热键）。
 *
 * 热键是独占资源，双击 Ctrl 也可能因为钩子装不上而失效，所以界面必须显示
 * 「真正生效的那个」，而不是把默认值硬编码到文案里。
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
    return () => {
      disposed = true;
    };
  }, [setSettings]);
}
