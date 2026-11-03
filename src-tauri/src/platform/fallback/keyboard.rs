//! 非 Windows 平台暂不支持键盘观察。双击 Ctrl 会退化为「只能用热键」。

use crate::platform::keyboard::KeySignal;

pub(crate) fn watch(_callback: impl Fn(KeySignal) + Send + 'static) -> Result<(), String> {
    Err("当前平台暂不支持双击 Ctrl，请改用热键".to_string())
}
