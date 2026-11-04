//! det 推理：预处理 + 会话 + 概率图。

use std::sync::{Mutex, OnceLock};

use ndarray::Array4;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

use super::DetOutput;

/// det 输入长边上限（PaddleOCR 默认 960）。
const MAX_SIDE: f32 = 960.0;
/// 边长必须是 32 的倍数，否则模型的下采样会错位。
const SIZE_ALIGN: u32 = 32;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();

/// 懒加载 det 会话。首次调用会把模型读进内存（约 4.5 MB）。
fn session(model_path: &std::path::Path) -> Result<&'static Mutex<Session>, String> {
    if let Some(existing) = SESSION.get() {
        return Ok(existing);
    }
    let created = Session::builder()
        .map_err(|e| format!("创建 det 会话失败：{e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        .with_intra_threads(4)
        .map_err(|e| e.to_string())?
        .commit_from_file(model_path)
        .map_err(|e| format!("加载 det 模型失败（{}）：{e}", model_path.display()))?;
    let _ = SESSION.set(Mutex::new(created));
    SESSION
        .get()
        .ok_or_else(|| "det 会话初始化失败".to_string())
}

/// 计算对齐到 32 倍数的目标尺寸。
pub fn aligned_size(width: u32, height: u32) -> (u32, u32) {
    // **只缩不放。**
    //
    // 曾经在这里加过"长边小于 640 就放大"，出发点是小字号检测不到；
    // 但实测真正原因是**后处理按收缩核高度过滤**（见 `postprocess.rs`），
    // 修好顺序后小字号本来就正常，加放大反而让 488px 宽的行被切成两段（CER 0→0.125）。
    let long = width.max(height) as f32;
    let scale = if long > MAX_SIDE {
        MAX_SIDE / long
    } else {
        1.0
    };
    let align = |value: f32| -> u32 {
        let scaled = (value * scale).max(1.0) as u32;
        scaled.max(SIZE_ALIGN).div_ceil(SIZE_ALIGN) * SIZE_ALIGN
    };
    (align(width as f32), align(height as f32))
}

/// 跑一次 det，返回概率图与缩放信息。
pub fn run(
    model_path: &std::path::Path,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Result<DetOutput, String> {
    let (det_w, det_h) = aligned_size(width, height);

    let resized = image::imageops::resize(
        &image::RgbImage::from_raw(width, height, pixels.to_vec())
            .ok_or_else(|| "图像数据长度不匹配".to_string())?,
        det_w,
        det_h,
        image::imageops::FilterType::Triangle,
    );

    let mut input = Array4::<f32>::zeros((1, 3, det_h as usize, det_w as usize));
    for (x, y, pixel) in resized.enumerate_pixels() {
        for c in 0..3 {
            let value = pixel[c] as f32 / 255.0;
            input[[0, c, y as usize, x as usize]] = (value - MEAN[c]) / STD[c];
        }
    }

    let mut guard = session(model_path)?
        .lock()
        .map_err(|_| "det 会话被毒化".to_string())?;
    let outputs = guard
        .run(ort::inputs![
            "x" => Tensor::from_array(input).map_err(|e| e.to_string())?
        ])
        .map_err(|e| format!("det 推理失败：{e}"))?;

    let (shape, data) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
    if dims.len() != 4 {
        return Err(format!("det 输出维度异常：{dims:?}"));
    }

    if std::env::var("LEMMA_OCR_DEBUG").is_ok() {
        let max = data.iter().cloned().fold(f32::MIN, f32::max);
        let mean = data.iter().sum::<f32>() / data.len() as f32;
        let hot = data.iter().filter(|v| **v > 0.3).count();
        eprintln!(
            "[det] src={width}x{height} → {det_w}x{det_h} prob={}x{} max={max:.3} mean={mean:.4} >0.3={hot}",
            dims[3], dims[2]
        );
    }

    Ok(DetOutput {
        prob: data.to_vec(),
        width: dims[3],
        height: dims[2],
        // 概率图坐标 → 原图坐标
        scale_x: width as f32 / dims[3] as f32,
        scale_y: height as f32 / dims[2] as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_align_to_32() {
        let (w, h) = aligned_size(900, 300);
        assert_eq!(w % 32, 0);
        assert_eq!(h % 32, 0);
        assert!(w >= 900 && h >= 300, "不应缩到比原图还小：{w}x{h}");
    }

    #[test]
    fn large_images_are_capped() {
        let (w, h) = aligned_size(4000, 2000);
        assert!(w <= MAX_SIDE as u32 + 32, "长边应被压到 960 附近，实际 {w}");
        assert_eq!(w % 32, 0);
        assert_eq!(h % 32, 0);
    }

    #[test]
    fn tiny_images_still_align() {
        let (w, h) = aligned_size(10, 10);
        assert_eq!(w % 32, 0);
        assert_eq!(h % 32, 0);
        // 10px 的图会被放大到接近 MIN_SIDE（受 MAX_UPSCALE 限制）
        assert!(w >= 32, "实际 {w}");
    }

    #[test]
    fn mid_size_images_are_untouched() {
        // 长边已在 [MIN_SIDE, MAX_SIDE] 之间：不缩放
        let (w, h) = aligned_size(800, 300);
        assert_eq!((w, h), (800, 320));
    }
}
