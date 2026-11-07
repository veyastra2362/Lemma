//! M4a 探针：PP-OCRv4 det + rec 端到端跑通，量延迟与内存。
//!
//! 目标不是做好产品，是回答三个问题：
//! 1. ONNX Runtime + Paddle2ONNX 导出的模型能不能跑（能）；
//! 2. det/rec 各要多久（决定交互预算）；
//! 3. 后处理要自己写多少（连通域 + unclip + 框排序，占大头）。
//!
//! 这里的后处理是**探针级简化版**：轴对齐包围盒 + 简单外扩，够用来验证链路，
//! 不是最终实现（正式版要最小外接矩形 + DB 的 unclip 距离）。

use std::collections::VecDeque;
use std::time::Instant;

use ndarray::Array4;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

const DET_MAX_SIDE: f32 = 960.0;
const DET_THRESHOLD: f32 = 0.3;
const REC_HEIGHT: u32 = 48;
fn rec_max_width() -> u32 { std::env::var("REC_MAX_WIDTH").ok().and_then(|v| v.parse().ok()).unwrap_or(320) }

struct Box2 {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

/// 当前进程工作集（MB）。用 sysinfo 读，口径与 docs/PERF.md 一致（含 mmap 页）。
fn rss_mb() -> f64 {
    use sysinfo::{Pid, System};
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let pid = Pid::from_u32(std::process::id());
    system
        .process(pid)
        .map(|p| p.memory() as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dir = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "data/ocr-models".to_string());
    let image_path = args.get(2).cloned().expect("需要图片路径");

    let image = image::open(&image_path)?.to_rgb8();
    let (img_w, img_h) = (image.width(), image.height());
    println!("输入图片：{img_w}x{img_h}");

    // ---------- det ----------
    let (w, h) = (img_w as f32, img_h as f32);
    let scale = (DET_MAX_SIDE / w.max(h)).min(1.0);
    let det_w = (((w * scale / 32.0).ceil() as u32) * 32).max(32) as usize;
    let det_h = (((h * scale / 32.0).ceil() as u32) * 32).max(32) as usize;
    let resized = image::imageops::resize(
        &image,
        det_w as u32,
        det_h as u32,
        image::imageops::FilterType::Triangle,
    );

    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let mut det_input = Array4::<f32>::zeros((1, 3, det_h, det_w));
    for (x, y, pixel) in resized.enumerate_pixels() {
        for c in 0..3 {
            let value = pixel[c] as f32 / 255.0;
            det_input[[0, c, y as usize, x as usize]] = (value - MEAN[c]) / STD[c];
        }
    }

    let det_file = match std::env::var("DET_MODEL").as_deref() {
        Ok("server") => "det_server.onnx",
        _ => "det.onnx",
    };
    println!("det 模型：{det_file}   基线 RSS：{:.1} MB", rss_mb());

    let t_load = Instant::now();
    let mut det = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(4)?
        .commit_from_file(format!("{dir}/{det_file}"))?;
    let mut rec = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(4)?
        .commit_from_file(format!("{dir}/rec.onnx"))?;
    println!(
        "两个会话创建：{:.0} ms（含加载与图优化）  RSS={:.1} MB",
        t_load.elapsed().as_secs_f64() * 1000.0,
        rss_mb()
    );

    // 模型切换成本：再建一个另一种 det 会话，量增量时间与内存
    if std::env::var("MEASURE_SWITCH").is_ok() {
        let other = if det_file == "det.onnx" { "det_server.onnx" } else { "det.onnx" };
        let t_switch = Instant::now();
        let _alt = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(format!("{dir}/{other}"))?;
        println!(
            "切到 {other} 需 {:.0} ms，切换后 RSS={:.1} MB",
            t_switch.elapsed().as_secs_f64() * 1000.0,
            rss_mb()
        );
    }

    let t_det = Instant::now();
    let det_out = det.run(ort::inputs!["x" => Tensor::from_array(det_input)?])?;
    let (det_shape, det_data) = det_out[0].try_extract_tensor::<f32>()?;
    let dims: Vec<usize> = det_shape.iter().map(|d| *d as usize).collect();
    let det_ms = t_det.elapsed().as_secs_f64() * 1000.0;
    println!("det：{det_ms:.0} ms  输出{dims:?}  RSS={:.1} MB", rss_mb());

    let prob_h = dims[2];
    let prob_w = dims[3];

    // ---------- 后处理：阈值 + 连通域 + 轴对齐包围盒 ----------
    let t_post = Instant::now();
    let boxes = components(det_data, prob_w, prob_h, det_w, det_h, img_w, img_h);
    let post_ms = t_post.elapsed().as_secs_f64() * 1000.0;
    println!("后处理：{post_ms:.0} ms  检出 {} 个文本框", boxes.len());

    // ---------- 识别 ----------
    let charset = std::fs::read_to_string(format!("{dir}/ppocr_keys_v1.txt"))?;
    let mut charset: Vec<char> = charset
        .lines()
        .map(|line| line.chars().next().unwrap_or(' '))
        .collect();
    // PaddleOCR 训练时 use_space_char=True：字符表末尾追加一个空格，
    // 模型输出类别数 = 字符表长度 + 1(blank) + 1(空格)。不补这个，空格会被 get 掉、整行连成一片。
    charset.push(' ');
    println!("字符表：{} 类（含末尾空格）", charset.len());

    let mut rec_total = 0.0;
    for (index, b) in boxes.iter().enumerate() {
        let crop_w = b.x1 - b.x0;
        let crop_h = b.y1 - b.y0;
        if crop_w < 8 || crop_h < 6 {
            continue;
        }
        let crop = image::imageops::crop_imm(&image, b.x0, b.y0, crop_w, crop_h).to_image();

        // rec 输入：高 48，宽按比例，右补零到 32 的倍数
        let ratio = crop_w as f32 / crop_h as f32;
        let target_w = ((REC_HEIGHT as f32 * ratio).ceil() as u32).clamp(16, rec_max_width());
        let resized = image::imageops::resize(
            &crop,
            target_w,
            REC_HEIGHT,
            image::imageops::FilterType::Triangle,
        );
        let padded_w = ((target_w + 31) / 32) * 32;
        let mut rec_input = Array4::<f32>::zeros((1, 3, REC_HEIGHT as usize, padded_w as usize));
        for (x, y, pixel) in resized.enumerate_pixels() {
            for c in 0..3 {
                rec_input[[0, c, y as usize, x as usize]] = pixel[c] as f32 / 127.5 - 1.0;
            }
        }

        let t_rec = Instant::now();
        let rec_out = rec.run(ort::inputs!["x" => Tensor::from_array(rec_input)?])?;
        rec_total += t_rec.elapsed().as_secs_f64() * 1000.0;
        let (shape, data) = rec_out[0].try_extract_tensor::<f32>()?;
        let shape: Vec<usize> = shape.iter().map(|d| *d as usize).collect();

        let text = greedy_decode(data, shape[1], shape[2], &charset);
        if index == 0 {
            println!("  （rec 输出形状 {shape:?}）");
        }        println!(
            "  [{index}] ({},{})-({},{})  {:.0}×{:.0}  → {text}",
            b.x0, b.y0, b.x1, b.y1, crop_w, crop_h
        );
    }
    println!("rec 合计：{rec_total:.0} ms  RSS={:.1} MB", rss_mb());

    Ok(())
}

/// 阈值 → 连通域 → 轴对齐包围盒（映射回原图坐标）。
fn components(
    prob: &[f32],
    prob_w: usize,
    prob_h: usize,
    det_w: usize,
    det_h: usize,
    img_w: u32,
    img_h: u32,
) -> Vec<Box2> {
    let mut visited = vec![false; prob_w * prob_h];
    let mut boxes = Vec::new();
    let mut queue = VecDeque::new();

    for y in 0..prob_h {
        for x in 0..prob_w {
            let index = y * prob_w + x;
            if visited[index] || prob[index] <= DET_THRESHOLD {
                continue;
            }

            let (mut min_x, mut min_y, mut max_x, mut max_y) = (x, y, x, y);
            let mut count = 0usize;
            visited[index] = true;
            queue.push_back((x, y));

            while let Some((cx, cy)) = queue.pop_front() {
                count += 1;
                min_x = min_x.min(cx);
                max_x = max_x.max(cx);
                min_y = min_y.min(cy);
                max_y = max_y.max(cy);

                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let nx = cx as i32 + dx;
                    let ny = cy as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= prob_w as i32 || ny >= prob_h as i32 {
                        continue;
                    }
                    let ni = ny as usize * prob_w + nx as usize;
                    if visited[ni] || prob[ni] <= DET_THRESHOLD {
                        continue;
                    }
                    visited[ni] = true;
                    queue.push_back((nx as usize, ny as usize));
                }
            }

            // 太小的当噪声
            if count < 12 {
                continue;
            }

            // DB 的 unclip 近似：按框高外扩（比例可用 PAD 环境变量调，用来验证"框太紧"的假设）
            let box_w = (max_x - min_x) as f32;
            let box_h = (max_y - min_y) as f32;
            let pad_ratio: f32 = std::env::var("PAD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.3);
            let pad = (box_h * pad_ratio).max(2.0);
            let sx = img_w as f32 / det_w as f32 * (det_w as f32 / prob_w as f32);
            let sy = img_h as f32 / det_h as f32 * (det_h as f32 / prob_h as f32);

            let x0 = ((min_x as f32 - pad).max(0.0) * sx) as u32;
            let y0 = ((min_y as f32 - pad).max(0.0) * sy) as u32;
            let x1 = (((max_x as f32 + pad) * sx) as u32).min(img_w);
            let y1 = (((max_y as f32 + pad) * sy) as u32).min(img_h);
            let _ = box_w;

            if x1 > x0 && y1 > y0 {
                boxes.push(Box2 { x0, y0, x1, y1 });
            }
        }
    }

    // 阅读顺序：先上后下，同一行先左后右
    boxes.sort_by(|a, b| {
        let row_a = a.y0 / 20;
        let row_b = b.y0 / 20;
        row_a.cmp(&row_b).then(a.x0.cmp(&b.x0))
    });
    boxes
}

/// CTC 贪心解码：每帧取最大值，去重、去 blank。
fn greedy_decode(data: &[f32], steps: usize, classes: usize, charset: &[char]) -> String {
    let mut out = String::new();
    let mut last = usize::MAX;
    for step in 0..steps {
        let base = step * classes;
        let mut best = 0usize;
        let mut best_score = f32::MIN;
        for c in 0..classes {
            let score = data[base + c];
            if score > best_score {
                best_score = score;
                best = c;
            }
        }
        // 0 是 CTC blank
        if best != 0 && best != last {
            // charset 下标从 1 开始对应实际字符（模型末位是 blank）
            if let Some(ch) = charset.get(best.saturating_sub(1)) {
                out.push(*ch);
            }
        }
        last = best;
    }
    out
}
