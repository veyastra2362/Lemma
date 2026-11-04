//! 音频播放。
//!
//! 用 Windows 自带的 `PlaySound`（winmm）而不是引 cpudio/rodio 那套：
//! - 我们只需要"把一段 WAV 播出来"，不需要混音、不需要低延迟音轨；
//! - `PlaySound` 是进程级的、异步、零依赖，失败也只是听不到声音，不影响功能。
//!
//! 注意：`PlaySound` 同时只能播一段，后一次调用会打断前一次——朗读场景下这正是想要的
//! （用户再点一次就是"重念"）。

/// 播放一个 WAV 文件（异步，立即返回）。
pub fn play_wav(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("音频文件不存在：{}", path.display()));
    }
    crate::platform::backend_play_wav(path)
}

/// 停止播放。
pub fn stop() {
    crate::platform::backend_stop_audio();
}

#[cfg(target_os = "windows")]
pub mod windows_impl {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_PURGE};

    fn wide(path: &std::path::Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn play(path: &std::path::Path) -> Result<(), String> {
        let buffer = wide(path);
        unsafe {
            PlaySoundW(
                windows::core::PCWSTR(buffer.as_ptr()),
                None,
                SND_FILENAME | SND_ASYNC,
            )
            .ok()
            .map_err(|error| format!("PlaySound 失败：{error}"))
        }
    }

    pub fn stop() {
        unsafe {
            let _ = PlaySoundW(windows::core::PCWSTR::null(), None, SND_PURGE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_rejected() {
        assert!(play_wav(std::path::Path::new("definitely-not-here.wav")).is_err());
    }
}
