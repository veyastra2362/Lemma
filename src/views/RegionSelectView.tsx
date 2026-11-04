import { useEffect, useRef, useState } from "react";

import { t } from "../i18n";
import { ipc, isTauri } from "../lib/ipc";

/** 一次拖拽出来的选区（CSS 像素，相对窗口）。 */
interface Selection {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * 框选层：铺满整个虚拟桌面，拖出一个矩形，交给 Rust 截屏 + OCR。
 *
 * 坐标要换成**物理像素**再传给 Rust：CSS 像素乘以 devicePixelRatio，
 * 再加上窗口左上角在虚拟桌面里的物理坐标（多屏时可为负）。
 */
export function RegionSelectView() {
  const [selection, setSelection] = useState<Selection | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const start = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void ipc.cancelRegionSelect();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const rectFrom = (
    from: { x: number; y: number },
    to: { x: number; y: number },
  ): Selection => ({
    x: Math.min(from.x, to.x),
    y: Math.min(from.y, to.y),
    width: Math.abs(to.x - from.x),
    height: Math.abs(to.y - from.y),
  });

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (busy || event.button !== 0) {
      return;
    }
    start.current = { x: event.clientX, y: event.clientY };
    setSelection({ x: event.clientX, y: event.clientY, width: 0, height: 0 });
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!start.current || busy) {
      return;
    }
    setSelection(rectFrom(start.current, { x: event.clientX, y: event.clientY }));
  };

  const finish = async (event: React.PointerEvent<HTMLDivElement>) => {
    if (!start.current || busy || !isTauri()) {
      return;
    }
    const from = start.current;
    start.current = null;
    const rect = rectFrom(from, { x: event.clientX, y: event.clientY });

    // 太小的拖拽当误触
    if (rect.width < 8 || rect.height < 8) {
      setSelection(null);
      return;
    }
    setSelection(rect);

    setBusy(true);
    setError(null);
    try {
      const scale = window.devicePixelRatio || 1;
      const origin = await ipc.regionWindowOrigin();
      await ipc.runRegionOcr({
        x: Math.round(origin.x + rect.x * scale),
        y: Math.round(origin.y + rect.y * scale),
        width: Math.round(rect.width * scale),
        height: Math.round(rect.height * scale),
      });
      // Rust 侧已经弹出 HUD，这里只要把遮罩收掉
      await ipc.cancelRegionSelect();
      setSelection(null);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setError(message);
      void ipc.logFrontend("error", "runRegionOcr failed: " + message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="relative h-full w-full cursor-crosshair select-none bg-black/25"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={(event) => void finish(event)}
      onContextMenu={(event) => {
        event.preventDefault();
        void ipc.cancelRegionSelect();
      }}
    >
      {selection && selection.width > 0 && (
        <div
          className="pointer-events-none absolute border border-[var(--hud-accent)] bg-[var(--hud-accent)]/10"
          style={{
            left: selection.x,
            top: selection.y,
            width: selection.width,
            height: selection.height,
          }}
        >
          <span className="absolute -top-5 left-0 rounded bg-black/70 px-1.5 py-0.5 font-mono text-[11px] text-white/85">
            {Math.round(selection.width)} × {Math.round(selection.height)}
          </span>
        </div>
      )}

      {!selection && !error && (
        <div className="pointer-events-none absolute left-1/2 top-8 -translate-x-1/2 rounded-lg border border-white/15 bg-black/70 px-4 py-2 text-[12px] text-white/85">
          {t("regionHint")}
        </div>
      )}

      {busy && (
        <div className="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-lg border border-white/15 bg-black/80 px-4 py-2 text-[12px] text-white/85">
          {t("regionBusy")}
        </div>
      )}

      {error && (
        <div className="absolute left-1/2 top-1/2 w-[min(480px,80vw)] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-red-400/40 bg-black/85 px-4 py-3 text-[12px] text-red-300">
          {t("regionFailed")}：{error}
        </div>
      )}
    </div>
  );
}
