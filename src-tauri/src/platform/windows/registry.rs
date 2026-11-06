//! 注册表：把 Native Messaging 宿主登记给 Chrome / Edge。
//!
//! 只用 `RegCreateKeyW` + `RegSetValueExW` 这两个最简 API：
//! 不需要 `SECURITY_ATTRIBUTES`，也就不必额外开 `Win32_Security` feature。

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegSetValueExW,
};
use windows::core::PCWSTR;

/// 把宿主登记到 Chrome 与 Edge（都在 HKCU，不需要管理员权限）。
///
/// 返回是否至少写成功一处——只装了其中一个浏览器也算成功。
pub(crate) fn register_native_host(name: &str, manifest_path: &str) -> bool {
    const ROOTS: [&str; 2] = [
        r"Software\Google\Chrome\NativeMessagingHosts",
        r"Software\Microsoft\Edge\NativeMessagingHosts",
    ];

    let mut any = false;
    for root in ROOTS {
        let key = format!(r"{root}\{name}");
        if write_default_value(&key, manifest_path) {
            any = true;
        }
    }
    if !any {
        crate::core::logs::line("registry: NativeMessagingHosts 写入失败（两个浏览器都没登记上）");
    }
    any
}

fn write_default_value(subkey: &str, value: &str) -> bool {
    let key_wide = to_wide(subkey);
    let data = to_wide(value);
    let empty = to_wide("");

    // SAFETY: 指针都指向本栈帧上以 NUL 结尾的宽字符串；句柄无论成功失败都会关闭。
    unsafe {
        let mut handle = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_wide.as_ptr()),
            Some(0),
            PCWSTR(empty.as_ptr()),
            REG_OPTION_NON_VOLATILE,
            windows::Win32::System::Registry::KEY_SET_VALUE,
            None,
            &mut handle,
            None,
        );
        if status != ERROR_SUCCESS {
            return false;
        }

        // 清单路径作为该键的**默认值**（名称是空串），类型 REG_SZ
        let bytes = std::slice::from_raw_parts(
            data.as_ptr() as *const u8,
            data.len() * std::mem::size_of::<u16>(),
        );
        let status = RegSetValueExW(handle, PCWSTR(empty.as_ptr()), Some(0), REG_SZ, Some(bytes));
        let _ = RegCloseKey(handle);
        status == ERROR_SUCCESS
    }
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
