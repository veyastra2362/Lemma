//! M5a 探针：Kokoro ONNX 能不能跑、跑多快、缺什么。
//!
//! 这台机器上没有 Rust 的音素化（phonemizer），所以这里用**任意合法 token 序列**
//! 只测模型本身的吞吐（RTF = 合成耗时 / 音频时长），音频内容无意义。

use std::time::Instant;

use ndarray::{Array1, Array2};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

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
    let model = args.get(1).cloned().expect("需要模型路径");
    println!("模型：{model}   基线 RSS={:.1} MB", rss_mb());

    let t0 = Instant::now();
    let mut session = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(4)?
        .commit_from_file(&model)?;
    println!(
        "会话创建：{:.0} ms   RSS={:.1} MB",
        t0.elapsed().as_secs_f64() * 1000.0,
        rss_mb()
    );

    for input in session.inputs.iter() {
        println!("  in  {} {:?}", input.name, input.input_type);
    }
    for output in session.outputs.iter() {
        println!("  out {} {:?}", output.name, output.output_type);
    }

    // 常见 Kokoro ONNX 输入：input_ids[i64], style[f32,1,256], speed[f32,1]
    let names: Vec<String> = session.inputs.iter().map(|i| i.name.clone()).collect();

    for token_count in [10usize, 30, 80] {
        // 随便一串合法范围内的 token（内容无意义，只看吞吐）
        let ids: Vec<i64> = (0..token_count).map(|i| ((i % 90) + 5) as i64).collect();
        let input_ids = Array2::<i64>::from_shape_vec((1, token_count), ids)?;
        let style = Array2::<f32>::from_shape_vec((1, 256), vec![0.5f32; 256])?;
        let speed = Array1::<f32>::from_vec(vec![1.0f32]);

        let t1 = Instant::now();
        let result = if names.iter().any(|n| n == "input_ids") {
            session.run(ort::inputs![
                "input_ids" => Tensor::from_array(input_ids)?,
                "style" => Tensor::from_array(style)?,
                "speed" => Tensor::from_array(speed)?
            ])
        } else if names.iter().any(|n| n == "tokens") {
            session.run(ort::inputs![
                "tokens" => Tensor::from_array(input_ids)?,
                "style" => Tensor::from_array(style)?,
                "speed" => Tensor::from_array(speed)?
            ])
        } else {
            println!("未知的输入约定：{names:?}");
            return Ok(());
        };
        let elapsed = t1.elapsed().as_secs_f64() * 1000.0;

        match result {
            Ok(outputs) => {
                let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
                let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
                let samples = data.len();
                // Kokoro 输出采样率 24000 Hz
                let audio_secs = samples as f64 / 24_000.0;
                println!(
                    "{token_count} tokens（当 {:.1}s 音频）：{elapsed:.0} ms  输出{dims:?}  RTF={:.2}x  RSS={:.1} MB",
                    audio_secs,
                    elapsed / 1000.0 / audio_secs.max(0.001),
                    rss_mb()
                );
            }
            Err(error) => println!("{token_count} tokens: 运行失败 {error}"),
        }
    }

    Ok(())
}
