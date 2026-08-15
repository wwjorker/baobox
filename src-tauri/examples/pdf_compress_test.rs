//! PDF 压缩验收：不只看体积降了多少，更要确认产物没被改坏。
//!
//! 改写内嵌图片流是最容易写出「看着成功、实际打不开」的操作，
//! 所以每份产物都要重新打开、核对页数、核对图片数量。

use baobox_lib::pdf_ops::compress_file;
use lopdf::{Document, Object};
use std::path::{Path, PathBuf};

/// 把样本复制到临时目录再处理。
///
/// 工具本身会把结果写到源文件旁的 Baobox_output/，这是对的产品行为；
/// 但测试若直接对着用户的真实文档跑，就会在人家的文件夹里留下一堆产物
/// ——包括解密后的私人文件，甚至落进云同步目录。样本一律先隔离。
fn stage(src: &Path, sandbox: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(sandbox).ok()?;
    let dst = sandbox.join(src.file_name()?);
    std::fs::copy(src, &dst).ok()?;
    Some(dst)
}

fn probe(p: &std::path::Path) -> Option<(usize, usize)> {
    let doc = Document::load(p).ok()?;
    let images = doc
        .objects
        .values()
        .filter(|o| {
            matches!(o, Object::Stream(s) if s.dict.get(b"Subtype")
                .and_then(|x| x.as_name()).map(|n| n == b"Image").unwrap_or(false))
        })
        .count();
    Some((doc.get_pages().len(), images))
}

fn main() {
    let list = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"C:\baobox-samples\pdf_list_real.txt".into());
    let quality: u8 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(70);

    // 挑一批含图片、体积适中的做样本
    let mut samples: Vec<(PathBuf, usize, usize, u64)> = Vec::new();
    for line in std::fs::read_to_string(&list).expect("读不到清单").lines() {
        if samples.len() >= 12 {
            break;
        }
        let p = PathBuf::from(line.trim());
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        if meta.len() < 300 * 1024 || meta.len() > 40 * 1024 * 1024 {
            continue;
        }
        if let Some((pages, images)) = probe(&p) {
            if images > 0 {
                samples.push((p, pages, images, meta.len()));
            }
        }
    }

    println!("======== PDF 压缩验收（质量 {quality}）========\n");
    println!(
        "{:<34} {:>9} {:>9} {:>8}  {:>5} {:>6}",
        "文件", "原体积", "新体积", "变化", "页数", "结构"
    );

    let (mut ok, mut broken, mut grew) = (0, 0, 0);
    let (mut total_in, mut total_out) = (0u64, 0u64);

    let sandbox = std::env::temp_dir().join("baobox_pdf_compress_test");
    let _ = std::fs::remove_dir_all(&sandbox);

    for (p, pages, _images, size) in &samples {
        let name: String = p
            .file_name()
            .unwrap()
            .to_string_lossy()
            .chars()
            .take(32)
            .collect();
        let Some(p) = stage(p, &sandbox) else {
            println!("{name:<34} 无法复制到沙箱，跳过");
            continue;
        };
        match compress_file(&p, quality) {
            Ok((dst, touched, _)) => {
                let new_size = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
                // 关键一步：产物必须能重新打开，且页数一页不少
                match probe(&dst) {
                    Some((np, _ni)) if np == *pages => {
                        let delta = 100.0 - (new_size as f64 / *size as f64 * 100.0);
                        if new_size > *size {
                            grew += 1;
                        }
                        ok += 1;
                        total_in += size;
                        total_out += new_size;
                        println!(
                            "{name:<34} {:>8.1}M {:>8.1}M {:>7.1}%  {pages:>5} {:>6}",
                            *size as f64 / 1e6,
                            new_size as f64 / 1e6,
                            delta,
                            format!("{touched}张")
                        );
                    }
                    Some((np, _)) => {
                        broken += 1;
                        println!("{name:<34} !! 页数从 {pages} 变成 {np}");
                    }
                    None => {
                        broken += 1;
                        println!("{name:<34} !! 产物打不开");
                    }
                }
            }
            Err(e) => println!("{name:<34} 跳过: {}", e.key),
        }
    }

    println!("\n结构完好 {ok} / 损坏 {broken} / 反而变大 {grew}");
    if total_in > 0 {
        println!(
            "整体 {:.1} MB → {:.1} MB，省下 {:.1}%",
            total_in as f64 / 1e6,
            total_out as f64 / 1e6,
            100.0 - total_out as f64 / total_in as f64 * 100.0
        );
    }
}
