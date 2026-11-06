//! Windows UI Automation 取词。
//!
//! 关键约束：**COM 是线程绑定的**。UI Automation 客户端必须在自己的 STA 线程上创建
//! 并调用，否则会拿到 `RPC_E_WRONG_THREAD` 或者直接把宿主线程卡住
//! （UIA 调用会阻塞等待目标应用的响应，某些应用没响应时要等好几秒）。
//!
//! 所以这里起一个常驻的 UIA 线程：STA 初始化 + 建一次 `IUIAutomation`，之后所有请求
//! 通过 channel 串行交给它，调用方在自己线程里等 reply。串行化同时也避免了
//! 「多个请求同时怼 UIA」造成的互相拖慢。
//!
//! `probe`（诊断）和 `capture`（真取词）共用同一套采集逻辑 [`gather`]，
//! 区别只在于 probe 额外探测控件支持哪些 pattern、并记录每档耗时。

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

use crate::platform::uia::{ElementInfo, ProbeReport, Selection, TierProbe, UiaOutcome};

const MAX_TEXT_CHARS: i32 = -1;

enum Job {
    Probe {
        point: Option<POINT>,
        reply: Sender<ProbeReport>,
    },
    Capture {
        reply: Sender<UiaOutcome>,
    },
    WriteProbe {
        reply: Sender<crate::platform::uia::WriteProbe>,
    },
}

static JOB_TX: OnceLock<Sender<Job>> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());
static REQUEST_SEQ: AtomicU32 = AtomicU32::new(0);

/// 一次采集的原始结果。
struct Gathered {
    cursor: POINT,
    element: ElementInfo,
    /// `Some` 表示拿到了文本范围（内容可能为空串）。
    selection: Option<String>,
    word: Option<String>,
    paragraph: String,
    selection_ms: f64,
    word_ms: f64,
    /// 只在 probe 路径上填；取词热路径不跑这 5 次跨进程查询。
    patterns: Vec<String>,
    /// 未命中原因 / 备注。
    note: String,
}

impl Gathered {
    /// 选区优先，其次光标所在词。两者都是空白则返回 None。
    fn best_text(&self) -> Option<&str> {
        self.selection
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .or_else(|| {
                self.word
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            })
    }
}

pub(crate) fn probe(point: Option<(i32, i32)>) -> ProbeReport {
    let Some(tx) = worker_checked() else {
        return ProbeReport {
            error: Some("UIA 线程不可用".to_string()),
            ..Default::default()
        };
    };

    let (reply_tx, reply_rx) = mpsc::channel();
    let seq = REQUEST_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    if let Err(error) = tx.send(Job::Probe {
        point: point.map(|(x, y)| POINT { x, y }),
        reply: reply_tx,
    }) {
        return ProbeReport {
            error: Some(format!("投递探测请求失败（#{seq}）：{error}")),
            ..Default::default()
        };
    }
    reply_rx.recv().unwrap_or_else(|error| ProbeReport {
        error: Some(format!("UIA 线程没有回话（#{seq}）：{error}")),
        ..Default::default()
    })
}

pub(crate) fn capture() -> UiaOutcome {
    let Some(tx) = worker_checked() else {
        return UiaOutcome {
            note: "UIA 线程不可用".to_string(),
            ..Default::default()
        };
    };

    let (reply_tx, reply_rx) = mpsc::channel();
    let seq = REQUEST_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    if let Err(error) = tx.send(Job::Capture { reply: reply_tx }) {
        return UiaOutcome {
            note: format!("投递取词请求失败（#{seq}）：{error}"),
            ..Default::default()
        };
    }
    reply_rx.recv().unwrap_or_else(|error| UiaOutcome {
        note: format!("UIA 线程没有回话（#{seq}）：{error}"),
        ..Default::default()
    })
}

fn worker_checked() -> Option<&'static Sender<Job>> {
    match worker() {
        Ok(tx) => Some(tx),
        Err(error) => {
            crate::core::logs::line(format!("uia worker failed: {error}"));
            None
        }
    }
}

fn worker() -> Result<&'static Sender<Job>, String> {
    if let Some(tx) = JOB_TX.get() {
        return Ok(tx);
    }

    let _guard = INIT_LOCK
        .lock()
        .map_err(|_| "UIA 初始化锁被污染".to_string())?;
    if let Some(tx) = JOB_TX.get() {
        return Ok(tx);
    }

    let (tx, rx) = mpsc::channel::<Job>();
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
            match job {
                Job::Probe { point, reply } => {
                    let _ = reply.send(run_probe(&automation, point));
                }
                Job::Capture { reply } => {
                    let _ = reply.send(run_capture(&automation));
                }
                Job::WriteProbe { reply } => {
                    let _ = reply.send(write_probe_on(&automation));
                }
            }
        }

        // SAFETY: 与上面的 CoInitializeEx 配对。
        unsafe { windows::Win32::System::Com::CoUninitialize() };
    });

    ready_rx
        .recv()
        .map_err(|_| "UIA 线程启动失败".to_string())??;

    let _ = JOB_TX.set(tx);
    Ok(JOB_TX.get().expect("刚刚设置过"))
}

fn run_capture(automation: &IUIAutomation) -> UiaOutcome {
    match gather(automation, None, false) {
        Ok(gathered) => {
            let app = gathered.element.process_name.clone();
            UiaOutcome {
                selection: gathered.selection.clone(),
                word: gathered.word.clone(),
                paragraph: gathered.paragraph.clone(),
                app,
                note: if !gathered.note.is_empty() {
                    gathered.note.clone()
                } else if gathered.best_text().is_some() {
                    String::new()
                } else {
                    "UIA 没拿到文本".to_string()
                },
            }
        }
        Err(error) => UiaOutcome {
            note: error,
            ..Default::default()
        },
    }
}

fn run_probe(automation: &IUIAutomation, point: Option<POINT>) -> ProbeReport {
    let gathered = match gather(automation, point, true) {
        Ok(gathered) => gathered,
        Err(error) => {
            return ProbeReport {
                tiers: vec![miss("T1-selection", &error), miss("T0-word", &error)],
                error: Some(error),
                ..Default::default()
            };
        }
    };

    // 光标位置要在 gather 之后从 Gathered 里取，这里不重复查询。
    let cursor = [gathered.cursor.x, gathered.cursor.y];

    ProbeReport {
        cursor,
        element: Some(gathered.element.clone()),
        patterns: gathered.patterns.clone(),
        tiers: vec![
            describe_tier("T1-selection", &gathered.selection, gathered.selection_ms),
            describe_tier("T0-word", &gathered.word, gathered.word_ms),
        ],
        context: Some(Selection {
            text: gathered.best_text().unwrap_or_default().to_string(),
            paragraph: gathered.paragraph.clone(),
        }),
        error: None,
    }
}

fn describe_tier(tier: &str, text: &Option<String>, elapsed_ms: f64) -> TierProbe {
    match text {
        Some(text) => {
            let trimmed = text.trim();
            TierProbe {
                tier: tier.to_string(),
                hit: !trimmed.is_empty(),
                chars: trimmed.chars().count(),
                elapsed_ms,
                detail: if trimmed.is_empty() {
                    "空".to_string()
                } else {
                    clip(trimmed)
                },
            }
        }
        None => miss(tier, "未拿到文本范围"),
    }
}

/// 采集核心：定位光标 → 找元素 → 取选区 / 光标词 / 段落。
fn gather(
    automation: &IUIAutomation,
    point: Option<POINT>,
    with_patterns: bool,
) -> Result<Gathered, String> {
    let cursor = match point {
        Some(point) => point,
        None => {
            let mut point = POINT::default();
            // SAFETY: point 是本栈帧上的有效可写结构。
            if unsafe { GetCursorPos(&mut point) }.is_err() {
                return Err("拿不到光标位置".to_string());
            }
            point
        }
    };

    // ---- T1：焦点元素上的选区 ----
    let started = Instant::now();
    let focused = unsafe { automation.GetFocusedElement() }.ok();
    let focused_info = focused.as_ref().map(describe);
    let focused_patterns = if with_patterns {
        focused.as_ref().map(detect_patterns).unwrap_or_default()
    } else {
        Vec::new()
    };
    let selection = focused.as_ref().and_then(|element| {
        // SAFETY: 不支持该 pattern 时返回错误，已被 ok() 吃掉。
        let pattern: IUIAutomationTextPattern =
            unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
        read_selection(&pattern).as_ref().map(range_text)
    });
    let selection_ms = started.elapsed().as_secs_f64() * 1000.0;

    // ---- T0：光标所在单词 ----
    let started = Instant::now();
    // SAFETY: 纯查询调用，无所有权转移。
    let cursor_element = unsafe { automation.ElementFromPoint(cursor) }.ok();
    let cursor_info = cursor_element.as_ref().map(describe);
    let cursor_patterns = if with_patterns {
        cursor_element
            .as_ref()
            .map(detect_patterns)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let word = cursor_element.as_ref().and_then(|element| {
        let pattern: IUIAutomationTextPattern =
            unsafe { element.GetCurrentPatternAs(UIA_TextPatternId) }.ok()?;
        range_from_point(&pattern, cursor).as_ref().map(range_text)
    });
    let word_ms = started.elapsed().as_secs_f64() * 1000.0;

    // ---- 段落：以命中的那一档为锚 ----
    let has_selection = selection
        .as_deref()
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false);

    let paragraph = if has_selection {
        paragraph_of(focused.as_ref(), None)
    } else {
        paragraph_of(cursor_element.as_ref(), Some(cursor))
    };

    let (element, patterns) = if has_selection {
        (focused_info, focused_patterns)
    } else {
        (cursor_info.or(focused_info), cursor_patterns)
    };

    let note = if element.is_none() {
        "拿不到任何 UIA 元素".to_string()
    } else {
        String::new()
    };

    Ok(Gathered {
        cursor,
        element: element.unwrap_or_default(),
        selection,
        word,
        paragraph,
        selection_ms,
        word_ms,
        patterns,
        note,
    })
}

/// 取锚点所在段落。锚点可以是焦点元素（读选区），也可以是光标下的元素（读光标词）。
fn paragraph_of(element: Option<&IUIAutomationElement>, point: Option<POINT>) -> String {
    let Some(element) = element else {
        return String::new();
    };
    // SAFETY: 不支持时返回 None。
    let Ok(pattern) =
        (unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) })
    else {
        return String::new();
    };

    let anchor = match point {
        Some(point) => range_from_point(&pattern, point),
        None => read_selection(&pattern),
    };

    anchor
        .as_ref()
        .map(|range| expand_text(range, TextUnit_Paragraph))
        .unwrap_or_default()
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

    ElementInfo {
        name: bstr(unsafe { element.CurrentName() }),
        class_name: bstr(unsafe { element.CurrentClassName() }),
        control_type: bstr(unsafe { element.CurrentLocalizedControlType() }),
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

/// 逐个试控件支持哪些 pattern（诊断用，只在 probe 路径上跑）。
pub(crate) fn detect_patterns(element: &IUIAutomationElement) -> Vec<String> {
    let mut found = Vec::new();

    macro_rules! probe_pattern {
        ($name:literal, $id:path, $iface:ty) => {
            // SAFETY: 不支持时返回错误，已被 is_ok 吃掉。
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
/// 回填可行性探针：只在**焦点控件**上查能力，不做任何写入。
pub(crate) fn uia_write_probe() -> crate::platform::uia::WriteProbe {
    let Some(tx) = worker_checked() else {
        return crate::platform::uia::WriteProbe {
            elapsed_ms: 0.0,
            ..Default::default()
        };
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    if tx.send(Job::WriteProbe { reply: reply_tx }).is_err() {
        return crate::platform::uia::WriteProbe {
            elapsed_ms: 0.0,
            ..Default::default()
        };
    }
    reply_rx.recv().unwrap_or_default()
}

/// 在 UIA 工作线程里跑（那台线程才有 `IUIAutomation` 实例）。
fn write_probe_on(automation: &IUIAutomation) -> crate::platform::uia::WriteProbe {
    use crate::platform::uia::WriteProbe;

    let started = Instant::now();
    let focused = (unsafe { automation.GetFocusedElement() })
        .ok()
        .map(|element| probe_target(&element));

    let mut point = POINT::default();
    // SAFETY: point 是本栈帧上的有效可写结构。
    let at_point = if unsafe { GetCursorPos(&mut point) }.is_ok() {
        (unsafe { automation.ElementFromPoint(point) })
            .ok()
            .map(|element| probe_target(&element))
    } else {
        None
    };

    WriteProbe {
        focused,
        at_point,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

/// 查一个控件能不能写、走哪条路。
fn probe_target(element: &IUIAutomationElement) -> crate::platform::uia::WriteTarget {
    use crate::platform::uia::WriteTarget;

    let mut target = WriteTarget {
        element: describe(element),
        patterns: detect_patterns(element),
        ..Default::default()
    };

    // ValuePattern：IsReadOnly = false 才能写
    if let Ok(value) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
    {
        target.value_readonly = unsafe { value.CurrentIsReadOnly() }
            .ok()
            .map(|flag| flag.as_bool());
        if let Ok(current) = unsafe { value.CurrentValue() } {
            let text: String = current.to_string();
            target.value_preview = text.chars().take(40).collect();
        }
    }
    target.legacy = target
        .patterns
        .iter()
        .any(|name| name == "LegacyIAccessible");

    // 选区长度：没有选区就没有"原地替换"的对象
    if let Ok(pattern) =
        unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        && let Some(range) = read_selection(&pattern)
    {
        target.selection_chars = range_text(&range).chars().count();
    }

    target.writable = if target.value_readonly == Some(false) {
        "value".to_string()
    } else if target.legacy && target.value_readonly.is_none() {
        "legacy".to_string()
    } else if target.patterns.iter().any(|name| name == "Text") {
        "text-only".to_string()
    } else {
        "none".to_string()
    };
    target
}
