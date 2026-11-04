//! 屏幕截取。
//!
//! 用 GDI 的 `BitBlt` 从屏幕 DC 直接拷像素：
//! - 简单、同步、不依赖 COM，对桌面截图足够；
//! - 代价是不支持 HDR / 受保护内容（DRM 窗口会是黑块），这类场景我们本来也不打算做。
//!
//! 输出一律是**物理像素**的 RGB 数据，坐标可为负（多屏副屏在主屏左侧/上方时）。

use serde::{Deserialize, Serialize};

/// 一块屏幕截图的原始像素。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screenshot {
    pub width: u32,
    pub height: u32,
    /// 紧密排列的 RGB 数据（长度 = width × height × 3）。
    #[serde(skip)]
    pub pixels: Vec<u8>,
}

/// 截取屏幕上的一块矩形（物理像素，允许负坐标）。
pub fn capture_region(x: i32, y: i32, width: u32, height: u32) -> Result<Screenshot, String> {
    if width == 0 || height == 0 {
        return Err("截图区域为空".to_string());
    }
    crate::platform::backend_capture_region(x, y, width, height)
}

#[cfg(target_os = "windows")]
pub mod windows_impl {
    use super::Screenshot;

    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
        CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ,
        ReleaseDC, SRCCOPY, SelectObject,
    };

    pub fn capture(x: i32, y: i32, width: u32, height: u32) -> Result<Screenshot, String> {
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err("拿不到屏幕 DC".to_string());
            }

            let memory = CreateCompatibleDC(Some(screen));
            if memory.is_invalid() {
                ReleaseDC(None, screen);
                return Err("创建内存 DC 失败".to_string());
            }

            let bitmap = CreateCompatibleBitmap(screen, width as i32, height as i32);
            if bitmap.is_invalid() {
                let _ = DeleteDC(memory);
                ReleaseDC(None, screen);
                return Err("创建位图失败".to_string());
            }

            let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
            // CAPTUREBLT 让分层窗口（部分工具提示、悬浮层）也进画面
            let blit = BitBlt(
                memory,
                0,
                0,
                width as i32,
                height as i32,
                Some(screen),
                x,
                y,
                SRCCOPY | CAPTUREBLT,
            );

            let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    // 负数高度 = 自上而下存储，省掉一次翻转
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let lines = GetDIBits(
                memory,
                bitmap,
                0,
                height,
                Some(buffer.as_mut_ptr() as *mut _),
                &mut info,
                DIB_RGB_COLORS,
            );

            SelectObject(memory, previous);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);

            if blit.is_err() {
                return Err("BitBlt 失败".to_string());
            }
            if lines == 0 {
                return Err("GetDIBits 没有取到像素".to_string());
            }

            // BGRA → RGB
            let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 3);
            for chunk in buffer.as_chunks::<4>().0 {
                pixels.push(chunk[2]);
                pixels.push(chunk[1]);
                pixels.push(chunk[0]);
            }

            Ok(Screenshot {
                width,
                height,
                pixels,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_region_is_rejected() {
        assert!(capture_region(0, 0, 0, 10).is_err());
        assert!(capture_region(0, 0, 10, 0).is_err());
    }
}
