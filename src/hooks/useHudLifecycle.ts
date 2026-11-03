import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { ipc, isTauri, onFirstPaint, onHudPresented } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import { useDeckStore } from "../store/useDeckStore";

/**
 * 窗口刚显示时焦点事件会有一次抖动，这段时间内的失焦不算用户离开。
 */
const BLUR_GRACE_MS = 250;

/**
 * HUD 生命周期：
 * - 收到 `hud://presented` 后在下一帧回报首帧，得到真实呼出延迟；
 * - Esc / 失焦收起浮层。
 */
export function useHudLifecycle(): void {
  const setLatencyMs = useAppStore((state) => state.setLatencyMs);
  const setCapture = useAppStore((state) => state.setCapture);
  const shownAt = useRef(0);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      event.preventDefault();
      if (!isTauri()) {
        return;
      }
      // 先退一层 Deck；已经到底层才关窗口。
      if (useDeckStore.getState().pop()) {
        return;
      }
      void ipc.hudHide();
    };
    window.addEventListener("keydown", onKeyDown);

    if (!isTauri()) {
      return () => window.removeEventListener("keydown", onKeyDown);
    }

    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = (unlisten: () => void) => {
      if (disposed) {
        unlisten();
      } else {
        unlisteners.push(unlisten);
      }
    };

    void onHudPresented((seq) => {
      shownAt.current = performance.now();
      requestAnimationFrame(() => {
        if (!disposed) {
          void ipc.reportFirstPaint(seq).catch(() => undefined);
        }
      });
      // 取词已经在 Rust 侧、呼出 HUD 之前完成，这里只是把它取回来渲染。
      void ipc
        .lastCapture()
        .then((capture) => {
          if (disposed) {
            return;
          }
          setCapture(capture);
          // 每次呼出都用新的取词结果重置整个卡片栈。
          if (capture) {
            useDeckStore.getState().reset(capture);
          }
        })
        .catch(() => undefined);
    }).then(keep);

    void onFirstPaint((sample) => setLatencyMs(sample.total_ms))
      .then(keep)
      .catch(() => undefined);

    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) {
          return;
        }
        if (performance.now() - shownAt.current < BLUR_GRACE_MS) {
          return;
        }
        void ipc.hudHide();
      })
      .then(keep);

    return () => {
      disposed = true;
      window.removeEventListener("keydown", onKeyDown);
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, [setLatencyMs, setCapture]);
}
