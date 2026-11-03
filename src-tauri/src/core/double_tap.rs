//! 「双击 Ctrl」判定。
//!
//! 纯逻辑，不碰任何 OS API，因此可以直接单测——这才是双击这种时序交互该有的样子。
//!
//! 规则（对齐主流翻译工具的手感）：
//! - 两次 Ctrl「按下→松开」之间的**松开间隔** ≤ `window` 才算双击；
//! - 单次按住超过 `hold_max` 视为长按（用户是在用 Ctrl 组合键），不计入；
//! - 中间按过任何其它键，状态立即复位（Ctrl+C 之后再敲一下 Ctrl 不算双击）；
//! - 三次连按只触发一次（触发后重新计数）。

use std::time::{Duration, Instant};

use crate::platform::keyboard::KeySignal;

/// 两次松开之间的最大间隔。
pub const DEFAULT_WINDOW: Duration = Duration::from_millis(320);

/// 单次按下超过这个时长算长按。
pub const DEFAULT_HOLD_MAX: Duration = Duration::from_millis(600);

pub struct DoubleTap {
    window: Duration,
    hold_max: Duration,
    /// 上一次「干净松开」的时刻。
    last_up: Option<Instant>,
    /// 本次按下开始的时刻。
    down_at: Option<Instant>,
}

impl Default for DoubleTap {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOW, DEFAULT_HOLD_MAX)
    }
}

impl DoubleTap {
    pub fn new(window: Duration, hold_max: Duration) -> Self {
        Self {
            window,
            hold_max,
            last_up: None,
            down_at: None,
        }
    }

    /// 喂进一个信号，返回 `true` 表示「双击成立，该呼出浮层了」。
    pub fn on_signal(&mut self, signal: KeySignal, now: Instant) -> bool {
        match signal {
            KeySignal::ControlDown => {
                // 自动重复（按住不放产生的重复 KeyDown）不重置计时。
                if self.down_at.is_none() {
                    self.down_at = Some(now);
                }
                false
            }
            KeySignal::ControlUp => {
                let Some(down_at) = self.down_at.take() else {
                    return false;
                };

                // 长按：用户在打 Ctrl 组合键，不参与双击判定。
                if now.saturating_duration_since(down_at) > self.hold_max {
                    self.last_up = None;
                    return false;
                }

                let hit = self
                    .last_up
                    .map(|previous| now.saturating_duration_since(previous) <= self.window)
                    .unwrap_or(false);

                // 无论是否命中都重新计时：连按三次只触发一次。
                self.last_up = if hit { None } else { Some(now) };
                hit
            }
            KeySignal::Other => {
                self.last_up = None;
                self.down_at = None;
                false
            }
        }
    }

    /// 双击间隔窗口（给上层做可配置项用）。
    pub fn window(&self) -> Duration {
        self.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用「毫秒偏移」构造时间点，测试读起来就是时间线。
    fn timeline() -> Instant {
        Instant::now()
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn press_release(detector: &mut DoubleTap, base: Instant, from: u64, to: u64) -> bool {
        assert!(!detector.on_signal(KeySignal::ControlDown, at(base, from)));
        detector.on_signal(KeySignal::ControlUp, at(base, to))
    }

    #[test]
    fn two_quick_taps_trigger_once() {
        let base = timeline();
        let mut detector = DoubleTap::default();

        assert!(!press_release(&mut detector, base, 0, 60));
        assert!(press_release(&mut detector, base, 180, 240));
    }

    #[test]
    fn a_slow_second_tap_does_not_trigger() {
        let base = timeline();
        let mut detector = DoubleTap::default();

        assert!(!press_release(&mut detector, base, 0, 60));
        // 松开后隔了 600ms 才再按，已经超出 320ms 窗口
        assert!(!press_release(&mut detector, base, 600, 660));
    }

    #[test]
    fn three_taps_only_fire_once() {
        let base = timeline();
        let mut detector = DoubleTap::default();

        assert!(!press_release(&mut detector, base, 0, 50));
        assert!(press_release(&mut detector, base, 120, 170));
        // 第三次应该重新从"第一击"开始算
        assert!(!press_release(&mut detector, base, 240, 290));
        assert!(press_release(&mut detector, base, 400, 450));
    }

    #[test]
    fn another_key_in_between_resets_the_sequence() {
        let base = timeline();
        let mut detector = DoubleTap::default();

        assert!(!press_release(&mut detector, base, 0, 50));
        assert!(!detector.on_signal(KeySignal::Other, at(base, 90)));
        // Ctrl+C 已经打断节奏，紧接着的 Ctrl 不能算第二击
        assert!(!press_release(&mut detector, base, 150, 200));
    }

    #[test]
    fn holding_ctrl_counts_as_a_combination_not_a_tap() {
        let base = timeline();
        let mut detector = DoubleTap::default();

        // 按住 900ms 再松：用户明显是在用 Ctrl 组合键
        assert!(!press_release(&mut detector, base, 0, 900));
        // 紧接着快速按一下也不该触发（上一次长按没有留下"第一击"）
        assert!(!press_release(&mut detector, base, 1000, 1040));
    }

    #[test]
    fn auto_repeat_does_not_break_the_pair() {
        let base = timeline();
        let mut detector = DoubleTap::default();

        assert!(!detector.on_signal(KeySignal::ControlDown, at(base, 0)));
        // 按住时系统会重复发 KeyDown
        assert!(!detector.on_signal(KeySignal::ControlDown, at(base, 30)));
        assert!(!detector.on_signal(KeySignal::ControlDown, at(base, 60)));
        assert!(!detector.on_signal(KeySignal::ControlUp, at(base, 70)));

        assert!(press_release(&mut detector, base, 150, 200));
    }

    #[test]
    fn an_up_without_a_down_is_ignored() {
        let base = timeline();
        let mut detector = DoubleTap::default();
        assert!(!detector.on_signal(KeySignal::ControlUp, at(base, 0)));
        assert!(!detector.on_signal(KeySignal::ControlUp, at(base, 100)));
    }
}
