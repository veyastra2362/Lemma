//! 端侧 OCR（M4b）：det + 后处理 + rec，全部在本进程内跑。
//!
//! 设计要点：
//! - **模型不进仓库**（`*.onnx` 被 .gitignore 挡掉），放 `%APPDATA%\dev.lunar.lemma\ocr-models\`；
//! - 会话**懒加载并常驻**：实测 mobile det+rec 共占用约 125 MB，关掉再建要重付 280 ms；
//! - 坐标系：对外一律用**原图像素坐标**，缩放只发生在内部；
//! - 纯逻辑（几何、后处理、解码）都在子模块里，可单测；这里只做编排。

pub mod det;
pub mod geometry;
pub mod postprocess;
pub mod rec;

use std::path::PathBuf;

use geometry::Quad;

/// 一段识别结果。
#[derive(Debug, Clone)]
pub struct TextBlock {
    pub text: String,
    /// 文本框四边形（原图坐标，左上→右上→右下→左下）。
    pub quad: Quad,
    /// 连通域像素数，用来粗略判断可信度。
    pub weight: usize,
}

/// det 的输出。
pub struct DetOutput {
    pub prob: Vec<f32>,
    pub width: usize,
    pub height: usize,
    pub scale_x: f32,
    pub scale_y: f32,
}

/// det 二值化阈值。
const DET_THRESHOLD: f32 = 0.3;
/// DB 的 unclip 比例。
///
/// **3.0 是基准集标定出来的**（见 `docs/OCR-CALIBRATION.md`）：
/// 1.5 平均 CER 0.136、2.5 是 0.008、3.0 是 **0.005**、4.0 回升到 0.009。
/// 偏小会把字形的上下沿切掉，偏大会把相邻行框进来。
const UNCLIP_RATIO: f32 = 3.0;
/// 小于这么多像素的连通域当噪声丢掉。
const MIN_BLOB_AREA: usize = 12;

/// OCR 模型目录。
pub fn model_dir() -> Option<PathBuf> {
    crate::core::paths::app_data_dir().map(|dir| dir.join("ocr-models"))
}

fn model_path(name: &str) -> Result<PathBuf, String> {
    let dir = model_dir().ok_or_else(|| "拿不到应用数据目录".to_string())?;
    let path = dir.join(name);
    if !path.exists() {
        return Err(format!(
            "缺少 OCR 模型：{}（把 det.onnx / rec.onnx / ppocr_keys_v1.txt 放进该目录）",
            path.display()
        ));
    }
    Ok(path)
}

/// 模型是否齐备（用于界面决定是否显示 OCR 入口）。
pub fn models_ready() -> bool {
    let Some(dir) = model_dir() else {
        return false;
    };
    ["det.onnx", "rec.onnx", "ppocr_keys_v1.txt"]
        .iter()
        .all(|name| dir.join(name).exists())
}

/// 对一张 RGB 图做整页 OCR。
///
/// `pixels` 是紧密排列的 RGB 数据（长度 = width × height × 3）。
pub fn recognize(pixels: &[u8], width: u32, height: u32) -> Result<Vec<TextBlock>, String> {
    recognize_scaled(pixels, width, height, 1.0)
}

/// 同 `recognize`，但会先按 `scale` 放大原图再识别。
///
/// 为什么需要：小字号（12–14 px）的文本框抠出来只有十来个像素高，放大到 rec 的 48 px
/// 是 3–4 倍插值，模型看到的已经糊了。先整体放大再检测，等价于"看更大的图"。
pub fn recognize_scaled(
    pixels: &[u8],
    width: u32,
    height: u32,
    scale: f32,
) -> Result<Vec<TextBlock>, String> {
    if scale > 1.01 {
        let image = image::RgbImage::from_raw(width, height, pixels.to_vec())
            .ok_or_else(|| "图像数据长度不匹配".to_string())?;
        let new_w = ((width as f32) * scale).round() as u32;
        let new_h = ((height as f32) * scale).round() as u32;
        let resized = image::imageops::resize(
            &image,
            new_w.max(1),
            new_h.max(1),
            image::imageops::FilterType::Lanczos3,
        );
        return recognize_inner(resized.as_raw(), new_w, new_h);
    }
    recognize_inner(pixels, width, height)
}

fn recognize_inner(pixels: &[u8], width: u32, height: u32) -> Result<Vec<TextBlock>, String> {
    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }
    if pixels.len() != (width as usize * height as usize * 3) {
        return Err(format!(
            "图像数据长度不匹配：期望 {}，实际 {}",
            width as usize * height as usize * 3,
            pixels.len()
        ));
    }

    // 标定旋钮：默认值是探针/基准集标定出来的，允许环境变量覆盖以便扫参数。
    let threshold = std::env::var("LEMMA_OCR_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DET_THRESHOLD);
    let unclip_ratio = std::env::var("LEMMA_OCR_UNCLIP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(UNCLIP_RATIO);

    let det_model = model_path("det.onnx")?;
    let rec_model = model_path("rec.onnx")?;
    let charset = model_path("ppocr_keys_v1.txt")?;

    let started = std::time::Instant::now();
    let output = det::run(&det_model, pixels, width, height)?;
    let det_ms = started.elapsed().as_secs_f64() * 1000.0;

    let candidates = postprocess::extract_boxes(postprocess::ExtractParams {
        prob: &output.prob,
        width: output.width,
        height: output.height,
        scale_x: output.scale_x,
        scale_y: output.scale_y,
        threshold,
        unclip_ratio,
        min_area: MIN_BLOB_AREA,
    });

    let rec_started = std::time::Instant::now();
    let mut blocks = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let text = rec::recognize(&rec_model, &charset, pixels, width, height, &candidate.quad)?;
        if text.trim().is_empty() {
            continue;
        }
        blocks.push(TextBlock {
            text,
            quad: candidate.quad,
            weight: candidate.area,
        });
    }
    let rec_ms = rec_started.elapsed().as_secs_f64() * 1000.0;

    crate::core::logs::line(format!(
        "ocr det={det_ms:.0}ms rec={rec_ms:.0}ms boxes={} chars={}",
        blocks.len(),
        blocks.iter().map(|b| b.text.chars().count()).sum::<usize>()
    ));

    Ok(blocks)
}

/// 编辑距离（Levenshtein），用来算字符错误率 CER。
///
/// 按字符（不是字节）比较——中文一个字算一个。
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b.len()]
}

/// 字符错误率：编辑距离 / 参考答案长度。0.0 表示完全正确。
pub fn char_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let length = reference.chars().count().max(1);
    edit_distance(reference, hypothesis) as f64 / length as f64
}

/// 把识别结果拼成纯文本（按阅读顺序，行间换行）。
pub fn blocks_to_text(blocks: &[TextBlock]) -> String {
    blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_mismatched_buffers() {
        let error = recognize(&[0u8; 10], 4, 4).unwrap_err();
        assert!(error.contains("长度不匹配"), "实际错误：{error}");
    }

    #[test]
    fn empty_image_yields_nothing() {
        assert!(recognize(&[], 0, 0).unwrap().is_empty());
    }

    #[test]
    fn joins_blocks_with_newlines() {
        let block = |text: &str| TextBlock {
            text: text.to_string(),
            quad: [
                geometry::Point::new(0.0, 0.0),
                geometry::Point::new(1.0, 0.0),
                geometry::Point::new(1.0, 1.0),
                geometry::Point::new(0.0, 1.0),
            ],
            weight: 10,
        };
        let text = blocks_to_text(&[block("第一行"), block("second line")]);
        assert_eq!(text, "第一行\nsecond line");
    }
}
