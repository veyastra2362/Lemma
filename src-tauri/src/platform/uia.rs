//! UI Automation 取词契约。
//!
//! 三档降级，档位越靠前代价越小、副作用越少：
//!
//! | 档 | 机制 | 副作用 |
//! | --- | --- | --- |
//! | T1 选区 | `TextPattern.GetSelection()` | 无 |
//! | T0 光标词 | `TextPattern.RangeFromPoint()` + 扩到 `WordUnit` | 无 |
//! | T2 剪贴板 | 模拟 Ctrl+C 读回（M1b 实现） | 污染剪贴板、慢 |
//!
//! 这里只定义「探测」用到的类型；真正取词用 [`Selection`]。

use serde::{Deserialize, Serialize};

/// 一段文本及其上下文。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Selection {
    /// 命中的词 / 选区原文。
    pub text: String,
    /// 所在段落。
    pub paragraph: String,
}

/// 一次 UIA 取词的结果（不含剪贴板兜底）。档位选择留给 core 决定。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiaOutcome {
    /// `TextPattern.GetSelection()` 拿到的选区文本（空选区时为 None）。
    pub selection: Option<String>,
    /// `RangeFromPoint` + `WordUnit` 拿到的光标所在词。
    pub word: Option<String>,
    /// 锚点所在段落，用来切句子。
    pub paragraph: String,
    /// 命中的进程名，用于诊断。
    pub app: String,
    /// 未命中原因 / 备注。
    pub note: String,
}

/// 命中的控件信息，用于诊断与支持矩阵。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElementInfo {
    pub name: String,
    pub class_name: String,
    pub control_type: String,
    pub automation_id: String,
    pub process_id: u32,
    pub process_name: String,
    /// 控件自身的窗口句柄（0 = 不可用）。回填靠它定位 `EM_REPLACESEL` 的目标。
    pub hwnd: i64,
    /// 控件矩形（物理像素）。取不到时为 None。
    pub bounds: Option<[i32; 4]>,
}

/// 单档探测结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierProbe {
    /// `T1-selection` / `T0-word`
    pub tier: String,
    pub hit: bool,
    pub chars: usize,
    pub elapsed_ms: f64,
    /// 命中原文（截断到 120 字符）或未命中的原因。
    pub detail: String,
}

/// 一次完整探测的报告。既是诊断数据，也是 M1 的支持矩阵证据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeReport {
    pub cursor: [i32; 2],
    /// 光标所在的控件。
    pub element: Option<ElementInfo>,
    /// 这个控件实际支持哪些 UIA Pattern。降级策略怎么选全看这一行。
    pub patterns: Vec<String>,
    pub tiers: Vec<TierProbe>,
    /// 上下文捕获结果（句子 / 段落）。
    pub context: Option<Selection>,
    /// 整体失败原因（拿不到 UIA 客户端等）。
    pub error: Option<String>,
}
/// 回填可行性探针的结果。
///
/// M6 的「原地回填」要先回答一个问题：**这个控件能不能被程序写入**。
/// UI Automation 里能用来的只有 `ValuePattern.SetValue` 和
/// `LegacyIAccessiblePattern.SetValue`；`TextPattern` 的 range 是**只读**的
/// （ITextRangeProvider 根本没有 SetText），所以浏览器/Word 这类基于 TextPattern
/// 的控件只能退回「模拟输入」。
/// 单个控件的写入能力。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WriteTarget {
    pub element: ElementInfo,
    pub patterns: Vec<String>,
    pub value_readonly: Option<bool>,
    pub legacy: bool,
    pub selection_chars: usize,
    /// ValuePattern 的内容预览（前 40 字），用来核对"写入是否真的生效"。
    pub value_preview: String,
    /// `value` / `legacy` / `text-only` / `none`
    pub writable: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WriteProbe {
    /// `GetFocusedElement()` 拿到的控件。
    pub focused: Option<WriteTarget>,
    /// 光标位置下的控件（产品取词走的那条路）。
    pub at_point: Option<WriteTarget>,
    pub elapsed_ms: f64,
}
