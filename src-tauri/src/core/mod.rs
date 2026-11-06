//! 跨平台应用逻辑。禁止出现 `cfg(target_os)` 与原生 API。

pub mod bundle;
pub mod dict;
pub mod double_tap;
pub mod logs;
pub mod ocr;
pub mod paths;
pub mod perf;
pub mod reader;
pub mod segment;
pub mod selection;
pub mod settings;
pub mod shell;
pub mod text;
pub mod translate;
pub mod translate_cache;
pub mod tray;
pub mod trigger;
pub mod tts;
