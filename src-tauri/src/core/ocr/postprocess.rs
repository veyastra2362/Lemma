//! det 概率图 → 文本框：阈值 + 连通域标记 + 最小外接矩形。

use super::geometry::{Point, Quad, min_area_rect, unclip};

/// 一个候选文本框。
#[derive(Debug, Clone)]
pub struct Candidate {
    /// 外扩后的四边形（顺序：左上→右上→右下→左下）。
    pub quad: Quad,
    /// 连通域像素数的粗略置信度（越大越可信）。
    pub area: usize,
}

/// 把 det 概率图转成候选框的全部参数。
pub struct ExtractParams<'a> {
    pub prob: &'a [f32],
    pub width: usize,
    pub height: usize,
    /// 概率图坐标 → 原图坐标的缩放。
    pub scale_x: f32,
    pub scale_y: f32,
    pub threshold: f32,
    pub unclip_ratio: f32,
    pub min_area: usize,
}

/// 阈值 → 连通域 → 最小外接矩形 → unclip。
pub fn extract_boxes(params: ExtractParams<'_>) -> Vec<Candidate> {
    let ExtractParams {
        prob,
        width,
        height,
        scale_x,
        scale_y,
        threshold,
        unclip_ratio,
        min_area,
    } = params;
    let mut visited = vec![false; width * height];
    let mut candidates = Vec::new();
    let mut stack: Vec<usize> = Vec::new();

    for start in 0..width * height {
        if visited[start] || prob[start] <= threshold {
            continue;
        }

        // 四邻域洪泛
        let mut points: Vec<Point> = Vec::new();
        visited[start] = true;
        stack.push(start);

        while let Some(index) = stack.pop() {
            let x = index % width;
            let y = index / width;
            points.push(Point::new(x as f32, y as f32));

            if x > 0 {
                let next = index - 1;
                if !visited[next] && prob[next] > threshold {
                    visited[next] = true;
                    stack.push(next);
                }
            }
            if x + 1 < width {
                let next = index + 1;
                if !visited[next] && prob[next] > threshold {
                    visited[next] = true;
                    stack.push(next);
                }
            }
            if y > 0 {
                let next = index - width;
                if !visited[next] && prob[next] > threshold {
                    visited[next] = true;
                    stack.push(next);
                }
            }
            if y + 1 < height {
                let next = index + width;
                if !visited[next] && prob[next] > threshold {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }

        if points.len() < min_area {
            continue;
        }

        // 概率图坐标 → 原图坐标
        let scaled: Vec<Point> = points
            .iter()
            .map(|p| Point::new(p.x * scale_x, p.y * scale_y))
            .collect();

        // 诊断开关：强制用轴对齐框。屏幕文本几乎都是水平的，
        // 而 min_area_rect 会因凸包边缘的离散抖动给出轻微旋转，导致框内文字被斜切。
        let quad = if std::env::var("LEMMA_OCR_AXIS_ALIGNED").is_ok() {
            let mut min_x = f32::MAX;
            let mut min_y = f32::MAX;
            let mut max_x = f32::MIN;
            let mut max_y = f32::MIN;
            for p in &scaled {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
                max_x = max_x.max(p.x);
                max_y = max_y.max(p.y);
            }
            [
                Point::new(min_x, min_y),
                Point::new(max_x, min_y),
                Point::new(max_x, max_y),
                Point::new(min_x, max_y),
            ]
        } else {
            let Some(quad) = min_area_rect(&scaled) else {
                continue;
            };
            quad
        };
        // 过滤过扁/过小的框（多为噪声或分隔线）
        let width_edge = ((quad[1].x - quad[0].x).powi(2) + (quad[1].y - quad[0].y).powi(2)).sqrt();
        let height_edge =
            ((quad[2].x - quad[1].x).powi(2) + (quad[2].y - quad[1].y).powi(2)).sqrt();
        if width_edge < 8.0 || height_edge < 6.0 {
            continue;
        }

        candidates.push(Candidate {
            quad: unclip(&quad, unclip_ratio),
            area: points.len(),
        });
    }

    sort_reading_order(&mut candidates);
    candidates
}

/// 按阅读顺序排序：先按行（y 接近的算同一行），行内从左到右。
pub fn sort_reading_order(boxes: &mut [Candidate]) {
    if boxes.is_empty() {
        return;
    }

    // 用平均框高的一半作为"同一行"的容差，比固定阈值稳
    let avg_height: f32 = boxes
        .iter()
        .map(|b| ((b.quad[2].y - b.quad[1].y).powi(2) + (b.quad[2].x - b.quad[1].x).powi(2)).sqrt())
        .sum::<f32>()
        / boxes.len() as f32;
    let tolerance = (avg_height * 0.5).max(4.0);

    boxes.sort_by(|a, b| {
        let ay = a.quad[0].y;
        let by = b.quad[0].y;
        if (ay - by).abs() <= tolerance {
            a.quad[0]
                .x
                .partial_cmp(&b.quad[0].x)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张只有若干"实心矩形"的概率图。
    fn prob_map(width: usize, height: usize, boxes: &[(usize, usize, usize, usize)]) -> Vec<f32> {
        let mut prob = vec![0.0f32; width * height];
        for &(x0, y0, x1, y1) in boxes {
            for y in y0..y1 {
                for x in x0..x1 {
                    prob[y * width + x] = 1.0;
                }
            }
        }
        prob
    }

    #[test]
    fn finds_one_box_per_blob() {
        let prob = prob_map(100, 50, &[(10, 10, 40, 20), (10, 30, 60, 40)]);
        let boxes = extract_boxes(ExtractParams {
            prob: &prob,
            width: 100,
            height: 50,
            scale_x: 1.0,
            scale_y: 1.0,
            threshold: 0.3,
            unclip_ratio: 0.0,
            min_area: 10,
        });
        assert_eq!(boxes.len(), 2);
    }

    #[test]
    fn tiny_blobs_are_dropped() {
        let prob = prob_map(100, 50, &[(10, 10, 12, 12)]);
        let boxes = extract_boxes(ExtractParams {
            prob: &prob,
            width: 100,
            height: 50,
            scale_x: 1.0,
            scale_y: 1.0,
            threshold: 0.3,
            unclip_ratio: 0.0,
            min_area: 10,
        });
        assert!(boxes.is_empty(), "3×3 的小块应被 min_area 过滤");
    }

    #[test]
    fn reading_order_is_top_to_bottom_then_left_to_right() {
        // 故意乱序给：右下、左下、上
        let prob = prob_map(
            200,
            200,
            &[(120, 100, 180, 120), (10, 100, 60, 120), (10, 10, 60, 30)],
        );
        let boxes = extract_boxes(ExtractParams {
            prob: &prob,
            width: 200,
            height: 200,
            scale_x: 1.0,
            scale_y: 1.0,
            threshold: 0.3,
            unclip_ratio: 0.0,
            min_area: 10,
        });
        assert_eq!(boxes.len(), 3);
        assert!(boxes[0].quad[0].y < 40.0, "第一块应是最上面那行");
        assert!(
            boxes[1].quad[0].x < boxes[2].quad[0].x,
            "同一行内应先左后右"
        );
    }

    #[test]
    fn scale_maps_back_to_source_coordinates() {
        let prob = prob_map(50, 25, &[(10, 10, 20, 20)]);
        // 概率图 50×25，原图 100×50 → 坐标翻倍
        let boxes = extract_boxes(ExtractParams {
            prob: &prob,
            width: 50,
            height: 25,
            scale_x: 2.0,
            scale_y: 2.0,
            threshold: 0.3,
            unclip_ratio: 0.0,
            min_area: 10,
        });
        assert_eq!(boxes.len(), 1);
        assert!(
            boxes[0].quad[0].x >= 19.0,
            "x 应被放大：{:?}",
            boxes[0].quad[0]
        );
    }

    #[test]
    fn sorting_tolerates_slight_vertical_jitter() {
        let mut boxes: Vec<Candidate> = vec![
            Candidate {
                quad: [
                    Point::new(100.0, 11.0),
                    Point::new(150.0, 11.0),
                    Point::new(150.0, 31.0),
                    Point::new(100.0, 31.0),
                ],
                area: 500,
            },
            Candidate {
                quad: [
                    Point::new(10.0, 10.0),
                    Point::new(60.0, 10.0),
                    Point::new(60.0, 30.0),
                    Point::new(10.0, 30.0),
                ],
                area: 500,
            },
        ];
        sort_reading_order(&mut boxes);
        assert!(boxes[0].quad[0].x < 50.0, "同一行内 y 差 1px 也应左边优先");
    }
}
