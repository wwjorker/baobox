//! 打码验收
//!
//! 唯一重要的问题：被遮住的像素**是不是真的没了**。
//! 网页上那种在原图上盖一层黑矩形的做法，原始数据还躺在文件里，
//! 随手就能扒出来——那不是打码，是让人以为自己安全了。
//!
//! 所以这里在图上写一段可识别的内容，打码后逐像素检查它是否已被改写，
//! 并且确认区域外的像素**没有**被误伤。

use baobox_lib::redact::{redact_image, RedactMode, Region};

fn main() {
    println!("======== 打码验收 ========\n");

    // 左半边填成细密的高频花纹当「敏感内容」。
    //
    // 第一版用的是纯色块——那是个错误的测试：均匀区域取平均后还是它自己，
    // 马赛克「看起来没改动任何像素」，但那不是缺陷，是均匀区本来就没有
    // 信息可销毁。要检验像素是否真被抹掉，内容必须是有变化的。
    let (w, h) = (400u32, 200u32);
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let p = if x < w / 2 {
                let v = (((x * 7 + y * 13) % 2) * 255) as u8;
                image::Rgba([v, 255 - v, (x % 251) as u8, 255])
            } else {
                image::Rgba([0, 0, 255, 255]) // 对照区，纯色即可
            };
            img.put_pixel(x, y, p);
        }
    }
    let before = img.clone();

    let regions = [Region {
        x: 0.0,
        y: 0.0,
        w: 0.5,
        h: 1.0,
    }];

    let mut pass = 0;
    let mut fail = 0;
    let mut check = |label: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {label:<20} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {label:<20} {detail}");
        }
    };

    for (name, mode) in [
        ("涂黑", RedactMode::Blackout),
        ("马赛克", RedactMode::Pixelate),
    ] {
        let mut work = before.clone();
        let n = redact_image(&mut work, &regions, mode);

        // 选区内：原始的鲜红必须一个像素都不剩
        let mut survivors = 0usize;
        for y in 0..h {
            for x in 0..w / 2 {
                if work.get_pixel(x, y) == before.get_pixel(x, y) {
                    survivors += 1;
                }
            }
        }
        let total = (h * w / 2) as usize;
        check(
            &format!("{name}·原像素销毁"),
            survivors * 100 / total < 5,
            format!("处理 {n} 处，选区内 {survivors}/{total} 个像素与原图相同"),
        );

        // 选区外：一个像素都不该被碰
        let mut collateral = 0usize;
        for y in 0..h {
            for x in w / 2..w {
                if work.get_pixel(x, y) != before.get_pixel(x, y) {
                    collateral += 1;
                }
            }
        }
        check(
            &format!("{name}·未误伤区外"),
            collateral == 0,
            format!("选区外被改动 {collateral} 个像素"),
        );
    }

    // 马赛克块不能太细：块太小就还能认出内容，等于没打
    let mut fine = before.clone();
    redact_image(&mut fine, &regions, RedactMode::Pixelate);
    let mut distinct = std::collections::HashSet::new();
    for y in 0..h {
        for x in 0..w / 2 {
            distinct.insert(fine.get_pixel(x, y).0);
        }
    }
    check(
        "马赛克粒度",
        distinct.len() < 64,
        format!("选区内只剩 {} 种颜色（越少越糊）", distinct.len()),
    );

    println!("\n======== 通过 {pass} / 失败 {fail} ========");
}
