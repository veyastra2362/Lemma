import { useEffect } from "react";

import { ipc, isTauri } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";

/**
 * 拉一次运行期信息：当前生效的呼出热键。
 *
 * 热键是独占资源，可能被别的软件占掉，所以界面必须显示「真正生效的那个」，
 * 而不是硬编码在文案里的默认值。
 */
export function useRuntimeInfo(): void {
  const setHotkey = useAppStore((state) => state.setHotkey);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let disposed = false;
    void ipc
      .activeHotkey()
      .then((hotkey) => {
        if (!disposed) {
          setHotkey(hotkey);
        }
      })
      .catch(() => {
        if (!disposed) {
          setHotkey(null);
        }
      });
    return () => {
      disposed = true;
    };
  }, [setHotkey]);
}
