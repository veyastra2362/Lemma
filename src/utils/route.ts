export type Route = "shell" | "settings" | "ocr-region";

const ROUTES: readonly Route[] = ["shell", "settings", "ocr-region"];

/**
 * 三个窗口共用同一个前端产物，靠 hash 区分：
 * `index.html#/shell` / `#/settings` / `#/ocr-region`。
 * 未知路由一律回落到 HUD，避免白屏。
 */
export function parseRoute(hash: string): Route {
  const name = hash
    .replace(/^#\/?/, "")
    .replace(/\/+$/, "")
    .trim()
    .toLowerCase();
  return (ROUTES as readonly string[]).includes(name) ? (name as Route) : "shell";
}
