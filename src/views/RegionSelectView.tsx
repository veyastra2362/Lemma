/**
 * 框选窗口占位。
 *
 * M4 会在这里实现：按下截图热键 → 冻结整个虚拟桌面为一张图铺满全屏 →
 * 拖拽出选区 → 按物理像素裁切 → 交给 OCR。现在只留一个可 Esc 退出的罩层，
 * 用于提前验证窗口透传与多屏负坐标。
 */
export function RegionSelectView() {
  return (
    <div className="flex h-full w-full items-center justify-center bg-black/25">
      <div className="rounded-lg border border-white/15 bg-black/60 px-4 py-2 text-[12px] text-white/80">
        框选功能将在 M4 接入 · 按 Esc 退出
      </div>
    </div>
  );
}
