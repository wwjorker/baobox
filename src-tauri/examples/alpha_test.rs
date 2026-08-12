//! 透明通道会不会被静默丢掉？
//!
//! JPEG 根本没有 alpha 通道。用户拿一张带透明背景的 PNG 去转 JPEG 或
//! 「压到指定体积」时，透明区域会变成什么？如果变成黑块而软件一声不吭，
//! 那就是静默的数据损坏——用户拿到的图和他期望的不是一回事。

use baobox_lib::image_ops::{compress_to_target, OutFmt};

fn main() {
    println!("======== 透明通道处理 ========\n");

    // 一张典型的「透明背景 + 中间有内容」的图，比如去了底的 logo
    let (w, h) = (300u32, 300u32);
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let inside = (x as i32 - 150).pow(2) + (y as i32 - 150).pow(2) < 80 * 80;
            *img.get_pixel_mut(x, y) = if inside {
                image::Rgba([230, 60, 40, 255]) // 不透明的红圆
            } else {
                image::Rgba([0, 0, 0, 0]) // 完全透明的背景
            };
        }
    }
    let src = image::DynamicImage::ImageRgba8(img);

    for (name, fmt) in [
        ("WebP", OutFmt::WebP),
        ("JPEG", OutFmt::Jpeg),
        ("PNG", OutFmt::Png),
    ] {
        match compress_to_target(&src, fmt, 200_000) {
            Ok(r) => {
                let out = image::load_from_memory(&r.bytes).expect("产物解不开");
                let rgba = out.to_rgba8();
                // 看原本完全透明的角落变成了什么
                let corner = rgba.get_pixel(5, 5).0;
                let keeps_alpha = corner[3] < 10;
                let is_black =
                    corner[0] < 30 && corner[1] < 30 && corner[2] < 30 && corner[3] > 200;
                println!(
                    "  {name:<5} {:>6} KB  角落像素 RGBA{:?}  {}",
                    r.bytes.len() / 1024,
                    corner,
                    if keeps_alpha {
                        "透明保留"
                    } else if is_black {
                        "!! 透明区变成了黑色"
                    } else {
                        "透明区被填成了某个颜色"
                    }
                );
            }
            Err(e) => println!("  {name:<5} 失败 {}", e.key),
        }
    }

    println!("\n结论：JPEG 无法承载透明通道。若软件不提示，用户会拿到一张");
    println!("背景被填成纯色的图，而他以为只是压小了体积。");
}
