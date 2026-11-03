//! 非 Windows 平台的 UIA 占位。M1 只覆盖 Windows，其它平台后面按契约补。

use crate::platform::uia::ProbeReport;

pub(crate) fn probe(_point: Option<(i32, i32)>) -> ProbeReport {
    ProbeReport {
        error: Some("当前平台未实现 UI Automation 取词".to_string()),
        ..Default::default()
    }
}
