import { create } from "zustand";

import type { Capture, RuntimeStats, Settings } from "../types";

interface AppState {
  stats: RuntimeStats | null;
  setStats: (stats: RuntimeStats) => void;
  /** 最近一次「热键 → 首帧」耗时，由 perf://first-paint 事件写入。 */
  latencyMs: number | null;
  setLatencyMs: (ms: number) => void;
  /** 当前生效的设置；null 表示还没读到。 */
  settings: Settings | null;
  setSettings: (settings: Settings) => void;
  /** 最近一次取词结果。 */
  capture: Capture | null;
  setCapture: (capture: Capture | null) => void;
}

export const useAppStore = create<AppState>((set) => ({
  stats: null,
  setStats: (stats) => set({ stats }),
  latencyMs: null,
  setLatencyMs: (latencyMs) => set({ latencyMs }),
  settings: null,
  setSettings: (settings) => set({ settings }),
  capture: null,
  setCapture: (capture) => set({ capture }),
}));
