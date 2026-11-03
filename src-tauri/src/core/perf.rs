//! 性能打点。
//!
//! M0 只测两件事，但这两件是整个项目所有 SLO 的基准：
//! - 冷启动到托盘就绪；
//! - 热键按下 → HUD 第一帧被绘制出来。
//!
//! 第二项必须由前端在 `requestAnimationFrame` 里回报，否则测到的只是
//! 「Rust 调用了 show()」，那数字没有意义。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PaintSample {
    /// 第几次呼出 HUD（从 1 开始）。
    pub seq: u64,
    /// 热键 → 首帧（毫秒）。
    pub total_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStats {
    pub startup_ms: f64,
    pub hud_presents: u64,
    pub last_first_paint_ms: Option<f64>,
}

pub struct Perf {
    started_at: Instant,
    hud_presents: AtomicU64,
    /// 最近一次呼出：(seq, 呼出时刻)。
    pending: Mutex<Option<(u64, Instant)>>,
    last_first_paint_ms: Mutex<Option<f64>>,
}

impl Default for Perf {
    fn default() -> Self {
        Self::new()
    }
}

impl Perf {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            hud_presents: AtomicU64::new(0),
            pending: Mutex::new(None),
            last_first_paint_ms: Mutex::new(None),
        }
    }

    pub fn startup_ms(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64() * 1000.0
    }

    /// 记录一次呼出，返回本次的序号。
    pub fn begin_present(&self) -> u64 {
        let seq = self.hud_presents.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut slot) = self.pending.lock() {
            *slot = Some((seq, Instant::now()));
        }
        seq
    }

    /// 前端回报首帧。序号对不上（用户连按两次热键）则丢弃，避免脏数据。
    pub fn finish_present(&self, seq: u64) -> Option<PaintSample> {
        let elapsed = {
            let mut slot = self.pending.lock().ok()?;
            match *slot {
                Some((pending_seq, at)) if pending_seq == seq => {
                    *slot = None;
                    at.elapsed()
                }
                _ => return None,
            }
        };

        let total_ms = elapsed.as_secs_f64() * 1000.0;
        if let Ok(mut last) = self.last_first_paint_ms.lock() {
            *last = Some(total_ms);
        }
        Some(PaintSample { seq, total_ms })
    }

    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            startup_ms: self.startup_ms(),
            hud_presents: self.hud_presents.load(Ordering::SeqCst),
            last_first_paint_ms: self.last_first_paint_ms.lock().ok().and_then(|v| *v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_or_unknown_sequences_are_ignored() {
        let perf = Perf::new();
        let seq = perf.begin_present();
        assert!(perf.finish_present(seq + 1).is_none());
        assert!(perf.finish_present(seq).is_some());
        // 同一序号不能重复结算
        assert!(perf.finish_present(seq).is_none());
    }

    #[test]
    fn a_newer_present_supersedes_the_older_one() {
        let perf = Perf::new();
        let first = perf.begin_present();
        let second = perf.begin_present();
        assert!(perf.finish_present(first).is_none());
        assert!(perf.finish_present(second).is_some());
        assert_eq!(perf.stats().hud_presents, 2);
    }

    #[test]
    fn stats_expose_the_last_sample() {
        let perf = Perf::new();
        let seq = perf.begin_present();
        perf.finish_present(seq);
        assert!(perf.stats().last_first_paint_ms.is_some());
    }
}
