//! DB 后处理需要的几何运算：凸包、最小外接矩形、unclip 外扩、四点多边形裁切。
//!
//! 为什么自己写：PaddleOCR 用 `pyclipper` 做多边形偏移、`cv2.minAreaRect` 求最小外接矩形。
//! 我们没有 OpenCV，也不想为了两个几何操作引一个几何库——这些算法本身不长，而且可以单测。

/// 一个点（图像坐标，x 向右、y 向下）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 四边形（顺序：左上 → 右上 → 右下 → 左下）。
pub type Quad = [Point; 4];

/// Andrew monotone chain 凸包。输入点会被排序，返回逆时针凸包。
pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut sorted: Vec<Point> = points.to_vec();
    sorted.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });

    let cross = |o: Point, a: Point, b: Point| -> f32 {
        (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
    };

    let mut lower: Vec<Point> = Vec::new();
    for &p in &sorted {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }

    let mut upper: Vec<Point> = Vec::new();
    for &p in sorted.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }

    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// 最小面积外接矩形（旋转卡壳）。
///
/// 返回 4 个角点，顺序为：左上、右上、右下、左下（按图像坐标系的"上"来判定）。
pub fn min_area_rect(points: &[Point]) -> Option<Quad> {
    let hull = convex_hull(points);
    if hull.len() < 3 {
        return None;
    }

    let mut best: Option<(f32, Quad)> = None;

    for i in 0..hull.len() {
        let a = hull[i];
        let b = hull[(i + 1) % hull.len()];
        let edge = Point::new(b.x - a.x, b.y - a.y);
        let length = (edge.x * edge.x + edge.y * edge.y).sqrt();
        if length < 1e-6 {
            continue;
        }
        let ux = edge.x / length;
        let uy = edge.y / length;

        // 把凸包投影到这组正交基上，得到轴对齐包围盒
        let (mut min_u, mut max_u) = (f32::MAX, f32::MIN);
        let (mut min_v, mut max_v) = (f32::MAX, f32::MIN);
        for p in &hull {
            let u = p.x * ux + p.y * uy;
            let v = -p.x * uy + p.y * ux;
            min_u = min_u.min(u);
            max_u = max_u.max(u);
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }

        let area = (max_u - min_u) * (max_v - min_v);
        if best
            .as_ref()
            .is_some_and(|(best_area, _)| area >= *best_area)
        {
            continue;
        }

        let to_image = |u: f32, v: f32| Point::new(u * ux - v * uy, u * uy + v * ux);
        let corners = [
            to_image(min_u, min_v),
            to_image(max_u, min_v),
            to_image(max_u, max_v),
            to_image(min_u, max_v),
        ];
        best = Some((area, order_quad(corners)));
    }

    best.map(|(_, quad)| quad)
}

/// 把任意顺序的四个角点整理成 左上 → 右上 → 右下 → 左下。
fn order_quad(points: [Point; 4]) -> Quad {
    // 左上 = x+y 最小；右下 = x+y 最大；右上 = x-y 最大；左下 = x-y 最小
    let mut ordered = points;
    let score_sum = |p: &Point| p.x + p.y;
    let score_diff = |p: &Point| p.x - p.y;

    let mut min_sum = 0;
    let mut max_sum = 0;
    let mut max_diff = 0;
    let mut min_diff = 0;
    for (i, p) in ordered.iter().enumerate() {
        if score_sum(p) < score_sum(&ordered[min_sum]) {
            min_sum = i;
        }
        if score_sum(p) > score_sum(&ordered[max_sum]) {
            max_sum = i;
        }
        if score_diff(p) > score_diff(&ordered[max_diff]) {
            max_diff = i;
        }
        if score_diff(p) < score_diff(&ordered[min_diff]) {
            min_diff = i;
        }
    }
    let _ = (&mut ordered, min_sum, max_sum, max_diff, min_diff);

    [
        points[min_sum],
        points[max_diff],
        points[max_sum],
        points[min_diff],
    ]
}

fn edge_length(a: Point, b: Point) -> f32 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

fn quad_center(quad: &Quad) -> Point {
    let x = quad.iter().map(|p| p.x).sum::<f32>() / 4.0;
    let y = quad.iter().map(|p| p.y).sum::<f32>() / 4.0;
    Point::new(x, y)
}

/// DB 的 unclip：把多边形沿外法线外扩 `distance`。
///
/// PaddleOCR 用 `pyclipper.PyclipperOffset`，这里做等价近似——矩形沿自身两条轴各扩 `distance`。
/// `distance = area × unclip_ratio / perimeter`（与 PaddleOCR 的公式一致）。
pub fn unclip(quad: &Quad, unclip_ratio: f32) -> Quad {
    let width = edge_length(quad[0], quad[1]);
    let height = edge_length(quad[1], quad[2]);
    let perimeter = (width + height) * 2.0;
    if perimeter <= 0.0 {
        return *quad;
    }
    let distance = (width * height) * unclip_ratio / perimeter;

    let center = quad_center(quad);
    let half_w = width / 2.0 + distance;
    let half_h = height / 2.0 + distance;

    // 用矩形自身的方向重建：u 轴沿上边，v 轴沿右边
    let ux_raw = quad[1].x - quad[0].x;
    let uy_raw = quad[1].y - quad[0].y;
    let u_len = (ux_raw * ux_raw + uy_raw * uy_raw).sqrt().max(1e-6);
    let ux = ux_raw / u_len;
    let uy = uy_raw / u_len;
    // 垂直方向（图像坐标系下顺时针 90°）
    let vx = -uy;
    let vy = ux;

    let corner = |su: f32, sv: f32| {
        Point::new(
            center.x + ux * half_w * su + vx * half_h * sv,
            center.y + uy * half_w * su + vy * half_h * sv,
        )
    };

    [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]
}

/// 按四点把图像"摆正"裁出来（等价于 PaddleOCR 的 `get_rotate_crop_image`）。
///
/// 用逆向映射 + 双线性采样：对输出图的每个像素，反解出它在原图里的位置去取值。
pub fn crop_quad(
    pixels: &[u8],
    width: u32,
    height: u32,
    quad: &Quad,
    out_height: u32,
    max_out_width: u32,
) -> Option<(Vec<u8>, u32, u32)> {
    let top = edge_length(quad[0], quad[1]).max(edge_length(quad[3], quad[2]));
    let bottom = edge_length(quad[3], quad[2]).max(edge_length(quad[0], quad[1]));
    let left = edge_length(quad[0], quad[3]).max(edge_length(quad[1], quad[2]));
    let right = edge_length(quad[1], quad[2]).max(edge_length(quad[0], quad[3]));

    let source_w = top.max(bottom).max(1.0);
    let source_h = left.max(right).max(1.0);

    // 先按目标高度缩放，宽度按比例；超长时按上限压缩（正式流程还会切段）
    let scale = out_height as f32 / source_h;
    let out_w = ((source_w * scale).round() as u32).clamp(1, max_out_width);
    let out_h = out_height;

    let mut out = vec![0u8; (out_w * out_h * 3) as usize];

    for oy in 0..out_h {
        let v = (oy as f32 + 0.5) / out_h as f32;
        for ox in 0..out_w {
            let u = (ox as f32 + 0.5) / out_w as f32;

            // 双线性插值四个角点
            let top_x = quad[0].x + (quad[1].x - quad[0].x) * u;
            let top_y = quad[0].y + (quad[1].y - quad[0].y) * u;
            let bottom_x = quad[3].x + (quad[2].x - quad[3].x) * u;
            let bottom_y = quad[3].y + (quad[2].y - quad[3].y) * u;
            let sx = top_x + (bottom_x - top_x) * v;
            let sy = top_y + (bottom_y - top_y) * v;

            if sx < 0.0 || sy < 0.0 || sx >= width as f32 || sy >= height as f32 {
                continue;
            }

            let x0 = sx as usize;
            let y0 = sy as usize;
            let x1 = (x0 + 1).min(width as usize - 1);
            let y1 = (y0 + 1).min(height as usize - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            let index = (oy * out_w + ox) as usize * 3;
            for c in 0..3 {
                let p00 = pixels[(y0 * width as usize + x0) * 3 + c] as f32;
                let p10 = pixels[(y0 * width as usize + x1) * 3 + c] as f32;
                let p01 = pixels[(y1 * width as usize + x0) * 3 + c] as f32;
                let p11 = pixels[(y1 * width as usize + x1) * 3 + c] as f32;
                let top_value = p00 * (1.0 - fx) + p10 * fx;
                let bottom_value = p01 * (1.0 - fx) + p11 * fx;
                out[index + c] = (top_value * (1.0 - fy) + bottom_value * fy).round() as u8;
            }
        }
    }

    Some((out, out_w, out_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.5
    }

    #[test]
    fn hull_of_a_square_is_the_square() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
            Point::new(5.0, 5.0), // 内部点应被剔除
        ];
        let hull = convex_hull(&points);
        assert_eq!(hull.len(), 4, "内部点不应进凸包");
    }

    #[test]
    fn min_area_rect_of_axis_aligned_box() {
        let points = vec![
            Point::new(10.0, 20.0),
            Point::new(110.0, 20.0),
            Point::new(110.0, 60.0),
            Point::new(10.0, 60.0),
        ];
        let quad = min_area_rect(&points).expect("应能求出矩形");
        assert!(
            approx(quad[0].x, 10.0) && approx(quad[0].y, 20.0),
            "左上是 {:?}",
            quad[0]
        );
        assert!(
            approx(quad[2].x, 110.0) && approx(quad[2].y, 60.0),
            "右下是 {:?}",
            quad[2]
        );
    }

    #[test]
    fn min_area_rect_is_smaller_than_the_axis_aligned_one_for_rotated_text() {
        // 一条 45° 倾斜的细长文本块
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(60.0, 60.0),
            Point::new(66.0, 54.0),
            Point::new(6.0, -6.0),
        ];
        let quad = min_area_rect(&points).expect("应能求出矩形");
        let area = edge_length(quad[0], quad[1]) * edge_length(quad[1], quad[2]);
        // 轴对齐包围盒是 66×66 ≈ 4356，旋转矩形应显著更小
        assert!(area < 1200.0, "最小外接矩形面积应远小于轴对齐，实际 {area}");
    }

    #[test]
    fn unclip_expands_the_rectangle() {
        let quad: Quad = [
            Point::new(10.0, 10.0),
            Point::new(110.0, 10.0),
            Point::new(110.0, 30.0),
            Point::new(10.0, 30.0),
        ];
        let expanded = unclip(&quad, 1.5);
        let width = edge_length(expanded[0], expanded[1]);
        let height = edge_length(expanded[1], expanded[2]);
        assert!(width > 100.0 && height > 20.0, "应向外扩：{width}×{height}");
        // 中心不变
        let center_before = quad_center(&quad);
        let center_after = quad_center(&expanded);
        assert!(approx(center_before.x, center_after.x) && approx(center_before.y, center_after.y));
    }

    #[test]
    fn crop_produces_requested_height() {
        // 20×10 的白图
        let pixels = vec![255u8; 20 * 10 * 3];
        let quad: Quad = [
            Point::new(0.0, 0.0),
            Point::new(20.0, 0.0),
            Point::new(20.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let (out, out_w, out_h) = crop_quad(&pixels, 20, 10, &quad, 48, 640).expect("应能裁切");
        assert_eq!(out_h, 48);
        assert_eq!(out_w, 96, "宽高比应保持 2:1");
        assert_eq!(out.len(), (out_w * out_h * 3) as usize);
        assert!(out.iter().all(|v| *v == 255), "纯白图裁出来还是白的");
    }
}
