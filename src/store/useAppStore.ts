import { create } from "zustand";

import type { RuntimeStats } from "../types";

interface AppState {
  stats: RuntimeStats | null;
  setStats: (stats: RuntimeStats) => void;
  /** 最近一次「热键 → 首帧」耗时，由 perf://first-paint 事件写入。 */
  latencyMs: number | null;
  setLatencyMs: (ms: number) => void;
}

export const useAppStore = create<AppState>((set) => ({
  stats: null,
  setStats: (stats) => set({ stats }),
  latencyMs: null,
  setLatencyMs: (latencyMs) => set({ latencyMs }),
}));
