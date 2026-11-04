//! rec 推理：裁图 + 会话 + CTC 贪心解码。

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use ndarray::Array4;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

use super::geometry::{Quad, crop_quad};

/// rec 输入高度（PP-OCR 固定 48）。
pub const REC_HEIGHT: u32 = 48;
/// rec 输入宽度上限。实测 640 明显优于 320 与 1920（见 docs/ONNX-PROBE.md）。
pub const REC_MAX_WIDTH: u32 = 640;
/// 超过这个比例的行会被切段识别，而不是压缩。
const SEGMENT_ASPECT: f32 = 640.0 / 48.0;

static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();
static CHARSET: OnceLock<Vec<char>> = OnceLock::new();

fn session(model_path: &Path) -> Result<&'static Mutex<Session>, String> {
    if let Some(existing) = SESSION.get() {
        return Ok(existing);
    }
    let created = Session::builder()
        .map_err(|e| format!("创建 rec 会话失败：{e}"))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| e.to_string())?
        .with_intra_threads(4)
        .map_err(|e| e.to_string())?
        .commit_from_file(model_path)
        .map_err(|e| format!("加载 rec 模型失败（{}）：{e}", model_path.display()))?;
    let _ = SESSION.set(Mutex::new(created));
    SESSION
        .get()
        .ok_or_else(|| "rec 会话初始化失败".to_string())
}

/// 读字符表。
///
/// **末尾必须补一个空格**：模型输出类别数 = 1(blank) + 字符表长度 + 1(空格)，
/// `ppocr_keys_v1.txt` 只有 6623 行，不补会让英文整行连成一片。
pub fn charset(path: &Path) -> Result<&'static [char], String> {
    if let Some(existing) = CHARSET.get() {
        return Ok(existing);
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("读取字符表失败（{}）：{e}", path.display()))?;
    let mut chars: Vec<char> = content
        .lines()
        .map(|line| line.chars().next().unwrap_or(' '))
        .collect();
    chars.push(' ');
    let _ = CHARSET.set(chars);
    CHARSET
        .get()
        .map(Vec::as_slice)
        .ok_or_else(|| "字符表加载失败".to_string())
}

/// 识别一个文本框。
pub fn recognize(
    model_path: &Path,
    charset_path: &Path,
    pixels: &[u8],
    width: u32,
    height: u32,
    quad: &Quad,
) -> Result<String, String> {
    let chars = charset(charset_path)?;

    // 过宽的行切段识别，避免压缩导致的漏字（探针实测压缩会丢字）
    let (top_len, _) = (
        ((quad[1].x - quad[0].x).powi(2) + (quad[1].y - quad[0].y).powi(2)).sqrt(),
        (),
    );
    let left_len = ((quad[3].x - quad[0].x).powi(2) + (quad[3].y - quad[0].y).powi(2)).sqrt();
    let aspect = top_len / left_len.max(1.0);
    // 目标：每段缩放到高 48 后宽度不超过 REC_MAX_WIDTH
    let segments = (aspect / SEGMENT_ASPECT).ceil().max(1.0) as usize;
    if std::env::var("LEMMA_OCR_DEBUG").is_ok() {
        eprintln!("[ocr] 行宽 {top_len:.0} 高 {left_len:.0} 宽高比 {aspect:.1} → 切 {segments} 段");
    }

    if segments <= 1 {
        return recognize_quad(model_path, chars, pixels, width, height, quad);
    }

    // 沿上边/下边等分切开。
    //
    // **不能加重叠**：重叠会让边界处的字被两段各识别一次，
    // 实测会拼出 `canononical` 这种重复（探针没分段所以没暴露）。
    // 边界切断偶尔丢半个字，比重字更容易接受，且正常字号下极少发生。
    let mut text = String::new();
    for segment in 0..segments {
        let start = segment as f32 / segments as f32;
        let end = (segment + 1) as f32 / segments as f32;
        let lerp =
            |a: crate::core::ocr::geometry::Point, b: crate::core::ocr::geometry::Point, t: f32| {
                crate::core::ocr::geometry::Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
            };
        let segment_quad: Quad = [
            lerp(quad[0], quad[1], start),
            lerp(quad[0], quad[1], end),
            lerp(quad[3], quad[2], end),
            lerp(quad[3], quad[2], start),
        ];
        let part = recognize_quad(model_path, chars, pixels, width, height, &segment_quad)?;
        text.push_str(&part);
    }
    Ok(text)
}

fn recognize_quad(
    model_path: &Path,
    chars: &[char],
    pixels: &[u8],
    width: u32,
    height: u32,
    quad: &Quad,
) -> Result<String, String> {
    let Some((crop, crop_w, crop_h)) =
        crop_quad(pixels, width, height, quad, REC_HEIGHT, REC_MAX_WIDTH)
    else {
        return Ok(String::new());
    };

    // 诊断：把模型实际看到的裁剪图写出来（LEMMA_OCR_DUMP=1）
    if std::env::var("LEMMA_OCR_DUMP").is_ok() {
        let dir = std::env::temp_dir().join("lemma-ocr-dump");
        let _ = std::fs::create_dir_all(&dir);
        let name = format!("crop-{}-{}.png", crop_w, crop_h);
        let _ = image::save_buffer(
            dir.join(name),
            &crop,
            crop_w,
            crop_h,
            image::ColorType::Rgb8,
        );
        eprintln!("[ocr] dump {crop_w}x{crop_h}");
    }

    let padded_w = crop_w.div_ceil(32) * 32;
    let mut input = Array4::<f32>::zeros((1, 3, crop_h as usize, padded_w as usize));
    for y in 0..crop_h as usize {
        for x in 0..crop_w as usize {
            let index = (y * crop_w as usize + x) * 3;
            for c in 0..3 {
                // 归一化到 [-1, 1]
                input[[0, c, y, x]] = crop[index + c] as f32 / 127.5 - 1.0;
            }
        }
    }

    let mut guard = session(model_path)?
        .lock()
        .map_err(|_| "rec 会话被毒化".to_string())?;
    let outputs = guard
        .run(ort::inputs![
            "x" => Tensor::from_array(input).map_err(|e| e.to_string())?
        ])
        .map_err(|e| format!("rec 推理失败：{e}"))?;

    let (shape, data) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
    if dims.len() != 3 {
        return Err(format!("rec 输出维度异常：{dims:?}"));
    }

    let text = greedy_decode(data, dims[1], dims[2], chars);
    if std::env::var("LEMMA_OCR_DEBUG").is_ok() {
        // 统计空格类（最后一个类）与 blank 的占比，用于确认解码是否真的拿到空格
        let classes = dims[2];
        let mut space_frames = 0usize;
        for step in 0..dims[1] {
            let base = step * classes;
            let mut best = 0usize;
            let mut best_score = f32::MIN;
            for class in 0..classes {
                if data[base + class] > best_score {
                    best_score = data[base + class];
                    best = class;
                }
            }
            if best == classes - 1 {
                space_frames += 1;
            }
        }
        eprintln!(
            "[ocr] rec dims={dims:?} classes={classes} charset={} 空格帧={space_frames} → {text:?}",
            chars.len()
        );
    }
    Ok(text)
}

/// CTC 贪心解码：逐帧取最大值，去掉 blank 与相邻重复。
pub fn greedy_decode(data: &[f32], steps: usize, classes: usize, charset: &[char]) -> String {
    let mut out = String::with_capacity(steps / 2);
    let mut last = usize::MAX;

    for step in 0..steps {
        let base = step * classes;
        let mut best = 0usize;
        let mut best_score = f32::MIN;
        for class in 0..classes {
            let score = data[base + class];
            if score > best_score {
                best_score = score;
                best = class;
            }
        }

        // 0 是 CTC blank；charset 下标 = class - 1
        if best != 0
            && best != last
            && let Some(ch) = best.checked_sub(1).and_then(|i| charset.get(i))
        {
            out.push(*ch);
        }
        last = best;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_hot(steps: usize, classes: usize, picks: &[usize]) -> Vec<f32> {
        let mut data = vec![0.0f32; steps * classes];
        for (step, class) in picks.iter().enumerate() {
            data[step * classes + class] = 1.0;
        }
        data
    }

    #[test]
    fn decodes_simple_sequence() {
        // blank(0), 'a'(1), blank(0), 'b'(2)
        let classes = 4;
        let data = one_hot(4, classes, &[0, 1, 0, 2]);
        let charset = vec!['a', 'b', 'c'];
        assert_eq!(greedy_decode(&data, 4, classes, &charset), "ab");
    }

    #[test]
    fn collapses_repeats() {
        let classes = 4;
        // 'a','a','a' 连续三帧 → 只保留一个
        let data = one_hot(3, classes, &[1, 1, 1]);
        let charset = vec!['a', 'b', 'c'];
        assert_eq!(greedy_decode(&data, 3, classes, &charset), "a");
    }

    #[test]
    fn blank_separates_repeated_characters() {
        let classes = 4;
        // 'a', blank, 'a' → "aa"
        let data = one_hot(3, classes, &[1, 0, 1]);
        let charset = vec!['a', 'b', 'c'];
        assert_eq!(greedy_decode(&data, 3, classes, &charset), "aa");
    }

    #[test]
    fn space_class_is_last_and_reachable() {
        // 类别数 = blank + 3 字符 + 空格 = 5
        let classes = 5;
        let charset = vec!['a', 'b', 'c', ' '];
        let data = one_hot(3, classes, &[1, 4, 2]);
        assert_eq!(greedy_decode(&data, 3, classes, &charset), "a b");
    }
}
