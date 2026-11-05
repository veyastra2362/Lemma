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
  /** 快捷键提示是否显示（设置里可关）。拖拽说明始终显示，否则用户不知道要做什么。 */
  const [showHints, setShowHints] = useState(true);
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    void ipc
      .getSettings()
      .then((value) => setShowHints(value.showHints))
      .catch(() => undefined);
  }, []);
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
      className="rg-root relative h-full w-full cursor-crosshair select-none"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={(event) => void finish(event)}
      onContextMenu={(event) => {
        event.preventDefault();
        void ipc.cancelRegionSelect();
      }}
    >
      <style>{REGION_STYLES}</style>

      {selection && selection.width > 0 && (
        <div
          className="pointer-events-none absolute"
          style={{
            left: selection.x,
            top: selection.y,
            width: selection.width,
            height: selection.height,
            border: "1.5px solid var(--hud-accent)",
            borderRadius: 2,
          }}
        >
          {/* 四角标：清楚但不夸张，8px 方块，只描边 */}
          <i className="rg-corner tl" />
          <i className="rg-corner tr" />
          <i className="rg-corner bl" />
          <i className="rg-corner br" />
          <span className="rg-size">
            {Math.round(selection.width)} × {Math.round(selection.height)}
          </span>
        </div>
      )}

      {!selection && !error && (
        <div className="rg-top">
          <span>{t("regionHint")}</span>
          {showHints && <span className="rg-top-sub">· {t("regionHintCancel")}</span>}
        </div>
      )}

      {/* 「正在识别…」已移除：整个过程只有 100–300ms，一闪而过反而干扰视线；
          截屏前也已经 flush 合成器，不会再把这个提示截进图里。 */}

      {error && (
        <div className="rg-error">
          {t("regionFailed")}：{error}
        </div>
      )}
    </div>
  );
}

/**
 * 框选层样式。
 *
 * 遮罩是**中性黑 45%**（不是蓝黑、不加模糊）；选区内部只略微提亮，
 * 让用户看清框住了什么；尺寸与提示都用等宽小字，不做发光与科技感描边。
 */
const REGION_STYLES = `
  .rg-root { background: rgba(0, 0, 0, 0.45); }
  .rg-corner {
    position: absolute; width: 7px; height: 7px;
    border: 1px solid var(--hud-accent);
    background: var(--hud-canvas);
  }
  .rg-corner.tl { left: -4px; top: -4px; }
  .rg-corner.tr { right: -4px; top: -4px; }
  .rg-corner.bl { left: -4px; bottom: -4px; }
  .rg-corner.br { right: -4px; bottom: -4px; }
  .rg-size {
    position: absolute; left: 0; top: -22px;
    font-family: var(--hud-mono); font-size: 10.5px; font-variant-numeric: tabular-nums;
    line-height: 17px; padding: 0 5px; border-radius: 3px;
    background: var(--hud-accent); color: var(--hud-canvas);
  }
  .rg-top {
    position: absolute; left: 50%; top: 22px; transform: translateX(-50%);
    display: inline-flex; gap: 6px; align-items: baseline;
    font-size: 12px; color: var(--hud-text);
    background: var(--hud-surface); border: 1px solid var(--hud-border);
    border-radius: var(--hud-r-ctl); padding: 4px 12px;
    white-space: nowrap;
  }
  .rg-top-sub { font-size: 11px; color: var(--hud-faint); }
  .rg-busy {
    position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
    display: inline-flex; align-items: center; gap: 8px;
    font-size: 12px; color: var(--hud-text);
    background: var(--hud-surface); border: 1px solid var(--hud-border);
    border-radius: var(--hud-r-ctl); padding: 6px 12px;
  }
  .rg-spinner {
    width: 12px; height: 12px; border-radius: 50%;
    border: 1.5px solid var(--hud-border); border-top-color: var(--hud-accent);
    animation: rg-spin 720ms linear infinite;
  }
  @keyframes rg-spin { to { transform: rotate(360deg); } }
  .rg-error {
    position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: min(480px, 80vw);
    font-size: 12px; line-height: 1.6; color: var(--hud-err);
    background: var(--hud-surface); border: 1px solid var(--hud-err);
    border-radius: var(--hud-r-ctl); padding: 10px 14px;
  }
  @media (prefers-reduced-motion: reduce) {
    .rg-spinner { animation-duration: 2.4s; }
  }
`;
