//! Windows UI Automation 取词。
//!
//! 关键约束：**COM 是线程绑定的**。UI Automation 客户端必须在自己的 STA 线程上创建
//! 并调用，否则会拿到 `RPC_E_WRONG_THREAD` 或者直接把宿主线程卡住
//! （UIA 调用会阻塞等待目标应用的响应，某些应用没响应时要等好几秒）。
//!
//! 所以这里起一个常驻的 UIA 线程：STA 初始化 + 建一次 `IUIAutomation`，之后所有请求
//! 通过 channel 串行交给它，调用方在主线程里等 reply。串行化同时也避免了
//! 「多个 IPC 命令同时怼 UIA」造成的互相拖慢。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use windows::Win32::Foundation::{CloseHandle, POINT, RECT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationLegacyIAccessiblePattern,
    IUIAutomationSelectionPattern, IUIAutomationTextPattern, IUIAutomationTextPattern2,
    IUIAutomationTextRange, IUIAutomationValuePattern, TextUnit, TextUnit_Paragraph, TextUnit_Word,
    UIA_LegacyIAccessiblePatternId, UIA_SelectionPatternId, UIA_TextPattern2Id, UIA_TextPatternId,
    UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::PWSTR;

use crate::platform::uia::{ElementInfo, ProbeReport, Selection, TierProbe};

/// UIA 调用可能阻塞，这里给它一个上限，避免用户感觉「按了没反应」。
const MAX_TEXT_CHARS: i32 = -1;

struct ProbeJob {
    point: Option<POINT>,
    reply: Sender<ProbeReport>,
}

static PROBE_TX: OnceLock<Sender<ProbeJob>> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());
static REQUEST_SEQ: AtomicU32 = AtomicU32::new(0);

/// 探测光标（或指定点）处的取词能力。
pub(crate) fn probe(point: Option<(i32, i32)>) -> ProbeReport {
    let tx = match worker() {
        Ok(tx) => tx,
        Err(error) => {
            return ProbeReport {
                error: Some(error),
                ..Default::default()
            };
        }
    };

    let (reply_tx, reply_rx) = mpsc::channel();
    let job = ProbeJob {
        point: point.map(|(x, y)| POINT { x, y }),
        reply: reply_tx,
    };
    let seq = REQUEST_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    if let Err(error) = tx.send(job) {
        return ProbeReport {
            error: Some(format!("UIA 线程不可用（请求 #{seq}）：{error}")),
            ..Default::default()
        };
    }
    reply_rx.recv().unwrap_or_else(|error| ProbeReport {
        error: Some(format!("UIA 线程没有回话（请求 #{seq}）：{error}")),
        ..Default::default()
    })
}

fn worker() -> Result<&'static Sender<ProbeJob>, String> {
    if let Some(tx) = PROBE_TX.get() {
        return Ok(tx);
    }

    let _guard = INIT_LOCK
        .lock()
        .map_err(|_| "UIA 初始化锁被污染".to_string())?;
    if let Some(tx) = PROBE_TX.get() {
        return Ok(tx);
    }

    let (tx, rx) = mpsc::channel::<ProbeJob>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    std::thread::spawn(move || {
        // SAFETY: 本线程只为 UIA 服务，COM 初始化与反初始化都归它自己。
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };

        // SAFETY: 标准 COM 实例化，CLSCTX_INPROC_SERVER 下 UIA 是进程内组件。
        let automation: IUIAutomation =
            match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
                Ok(automation) => automation,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("创建 UIA 客户端失败：{error}")));
                    return;
                }
            };

        let _ = ready_tx.send(Ok(()));

        for job in rx {
            let report = run_probe(&automation, job.point);
            let _ = job.reply.send(report);
        }

        // SAFETY: 与上面的 CoInitializeEx 配对。
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    });

    ready_rx
        .recv()
        .map_err(|_| "UIA 线程启动失败".to_string())??;

    let _ = PROBE_TX.set(tx);
    Ok(PROBE_TX.get().expect("刚刚设置过"))
}

fn run_probe(automation: &IUIAutomation, point: Option<POINT>) -> ProbeReport {
    let mut report = ProbeReport::default();

    let cursor = match point {
        Some(point) => point,
        None => {
            let mut point = POINT::default();
            // SAFETY: point 是本栈帧上的有效可写结构。
            if unsafe { GetCursorPos(&mut point) }.is_err() {
                report.error = Some("拿不到光标位置".to_string());
                return report;
            }
            point
        }
    };
    report.cursor = [cursor.x, cursor.y];

    // SAFETY: 纯查询调用，无所有权转移。
    let element = match unsafe { automation.ElementFromPoint(cursor) } {
        Ok(element) => element,
        Err(error) => {
            report.error = Some(format!("ElementFromPoint 失败：{error}"));
            return report;
        }
    };
    report.element = Some(describe(&element));
    report.patterns = detect_patterns(&element);

    // SAFETY: GetCurrentPatternAs 在控件不支持该 pattern 时返回错误，不会崩。
    let pattern: IUIAutomationTextPattern =
        match unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) } {
            Ok(pattern) => pattern,
            Err(error) => {
                let reason = format!(
                    "控件不支持 TextPattern（HRESULT 0x{:08X}）",
                    error.code().0 as u32
                );
                report.tiers.push(miss("T1-selection", &reason));
                report.tiers.push(miss("T0-word", &reason));
                return report;
            }
        };

    // ---- T1：用户已有选区 ----
    let started = Instant::now();
    let selection_range = read_selection(&pattern);
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let selection_text = selection_range.as_ref().map(range_text).unwrap_or_default();
    let trimmed = selection_text.trim();

    report.tiers.push(match selection_range.as_ref() {
        Some(_) => TierProbe {
            tier: "T1-selection".to_string(),
            hit: !trimmed.is_empty(),
            chars: trimmed.chars().count(),
            elapsed_ms: elapsed,
            detail: clip(trimmed),
        },
        None => miss("T1-selection", "没有选区"),
    });

    // ---- T0：光标所在单词 ----
    let started = Instant::now();
    let word_range = range_from_point(&pattern, cursor);
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    let word_text = word_range.as_ref().map(range_text).unwrap_or_default();
    let word_trimmed = word_text.trim();

    report.tiers.push(match word_range.as_ref() {
        Some(_) => TierProbe {
            tier: "T0-word".to_string(),
            hit: !word_trimmed.is_empty(),
            chars: word_trimmed.chars().count(),
            elapsed_ms: elapsed,
            detail: clip(word_trimmed),
        },
        None => miss("T0-word", "RangeFromPoint 失败或扩不到词"),
    });

    // ---- 上下文：以命中的 range 为锚点扩到句子 / 段落 ----
    let anchor = selection_range
        .filter(|_| !trimmed.is_empty())
        .or_else(|| word_range.filter(|_| !word_trimmed.is_empty()));

    if let Some(anchor) = anchor {
        report.context = Some(Selection {
            text: if trimmed.is_empty() {
                word_trimmed.to_string()
            } else {
                trimmed.to_string()
            },
            paragraph: expand_text(&anchor, TextUnit_Paragraph),
        });
    }

    report
}

fn miss(tier: &str, reason: &str) -> TierProbe {
    TierProbe {
        tier: tier.to_string(),
        hit: false,
        chars: 0,
        elapsed_ms: 0.0,
        detail: reason.to_string(),
    }
}

/// 逐个试控件支持哪些 pattern。
///
/// 这一步是「先量再写」的核心：不同应用暴露的 pattern 差别极大，
/// 有的只给 LegacyIAccessible（MSAA 桥），有的只给 Value 不给 Text。
fn detect_patterns(element: &IUIAutomationElement) -> Vec<String> {
    let mut found = Vec::new();

    macro_rules! probe_pattern {
        ($name:literal, $id:path, $iface:ty) => {
            // SAFETY: 不支持时返回错误，已被 match 吃掉。
            if unsafe { element.GetCurrentPatternAs::<$iface>($id) }.is_ok() {
                found.push($name.to_string());
            }
        };
    }

    probe_pattern!("Text", UIA_TextPatternId, IUIAutomationTextPattern);
    probe_pattern!("Text2", UIA_TextPattern2Id, IUIAutomationTextPattern2);
    probe_pattern!("Value", UIA_ValuePatternId, IUIAutomationValuePattern);
    probe_pattern!(
        "LegacyIAccessible",
        UIA_LegacyIAccessiblePatternId,
        IUIAutomationLegacyIAccessiblePattern
    );
    probe_pattern!(
        "Selection",
        UIA_SelectionPatternId,
        IUIAutomationSelectionPattern
    );

    found
}

fn clip(text: &str) -> String {
    let mut clipped: String = text.chars().take(120).collect();
    if text.chars().count() > 120 {
        clipped.push('…');
    }
    clipped
}

fn read_selection(pattern: &IUIAutomationTextPattern) -> Option<IUIAutomationTextRange> {
    // SAFETY: pattern 是从活的控件上取到的 pattern；空选区时 Length() 为 0。
    let ranges = unsafe { pattern.GetSelection() }.ok()?;
    let length = unsafe { ranges.Length() }.ok()?;
    if length <= 0 {
        return None;
    }
    unsafe { ranges.GetElement(0) }.ok()
}

fn range_from_point(
    pattern: &IUIAutomationTextPattern,
    point: POINT,
) -> Option<IUIAutomationTextRange> {
    // SAFETY: point 来自 GetCursorPos / 用户参数；点在控件外时返回错误。
    let range = unsafe { pattern.RangeFromPoint(point) }.ok()?;
    // SAFETY: 展开单位是纯查询，扩展失败则这一档算未命中。
    unsafe { range.ExpandToEnclosingUnit(TextUnit_Word) }.ok()?;
    Some(range)
}

fn expand_text(range: &IUIAutomationTextRange, unit: TextUnit) -> String {
    // SAFETY: Clone 出的新 range 归我们所有，随便展开。
    let Ok(clone) = (unsafe { range.Clone() }) else {
        return String::new();
    };
    if unsafe { clone.ExpandToEnclosingUnit(unit) }.is_err() {
        return String::new();
    }
    range_text(&clone).trim().to_string()
}

fn range_text(range: &IUIAutomationTextRange) -> String {
    // SAFETY: 纯读操作。
    unsafe { range.GetText(MAX_TEXT_CHARS) }
        .map(|text| text.to_string())
        .unwrap_or_default()
}

fn describe(element: &IUIAutomationElement) -> ElementInfo {
    // SAFETY: 全是 Current* 只读属性查询，失败就留空，不影响其它字段。
    let pid = unsafe { element.CurrentProcessId() }.unwrap_or(0) as u32;

    let class_name = bstr(unsafe { element.CurrentClassName() });
    let localized_type = bstr(unsafe { element.CurrentLocalizedControlType() });

    ElementInfo {
        name: bstr(unsafe { element.CurrentName() }),
        class_name,
        control_type: localized_type,
        automation_id: bstr(unsafe { element.CurrentAutomationId() }),
        process_id: pid,
        process_name: process_name(pid),
        bounds: unsafe { element.CurrentBoundingRectangle() }
            .ok()
            .map(|rect: RECT| [rect.left, rect.top, rect.right, rect.bottom]),
    }
}

fn bstr(value: windows::core::Result<windows::core::BSTR>) -> String {
    value.map(|text| text.to_string()).unwrap_or_default()
}

/// 从 PID 反查进程文件名（不含路径）。诊断用，拿不到就返回空串。
fn process_name(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }

    // SAFETY: OpenProcess 失败只返回错误；缓冲区生命周期覆盖这次调用。
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };

        let mut buffer = [0u16; 512];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);

        if result.is_err() || size == 0 {
            return String::new();
        }

        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or("").to_string()
    }
}
