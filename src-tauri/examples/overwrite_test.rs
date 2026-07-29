//! 重复运行不该在输出目录里堆积
//!
//! 「绝不覆盖」守的是用户的原文件。我们自己上一次的产物是另一回事——
//! 早先一律加 (2)(3) 后缀，同一批文件跑三遍就堆出三份，
//! 而用户想要的几乎总是最新那份。

fn main() {
    let dir = std::env::temp_dir().join("baobox_overwrite_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 一张普通图片
    let src = dir.join("sample.png");
    let mut img = image::RgbImage::new(200, 150);
    for p in img.pixels_mut() {
        *p = image::Rgb([240, 238, 232]);
    }
    image::DynamicImage::ImageRgb8(img).save(&src).unwrap();

    println!("======== 重复运行的输出行为 ========\n");

    for round in 1..=3 {
        let r = baobox_lib::watermark::watermark_file(&src, "测试水印", 0.4, false);
        let out = dir.join(baobox_lib::paths::OUTPUT_DIR);
        let n = std::fs::read_dir(&out).map(|d| d.count()).unwrap_or(0);
        println!(
            "  第 {round} 次运行后，输出目录里有 {n} 个文件  {}",
            match &r {
                Ok((p, _)) => format!("→ {}", p.file_name().unwrap().to_string_lossy()),
                Err(e) => format!("失败 {}", e.key),
            }
        );
    }

    let out = dir.join(baobox_lib::paths::OUTPUT_DIR);
    let files: Vec<String> = std::fs::read_dir(&out)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    // 原图必须一个字节都没变
    let orig = image::open(&src).unwrap().to_rgb8();
    let untouched = orig.pixels().all(|p| p.0 == [240, 238, 232]);

    println!();
    println!(
        "  最终文件: {:?}",
        files
    );
    println!(
        "\n======== {} ========",
        if files.len() == 1 && untouched {
            "通过：跑三遍只留一份最新结果，原图未被修改"
        } else if !untouched {
            "失败：原图被改动了"
        } else {
            "失败：输出仍在堆积"
        }
    );

    let _ = std::fs::remove_dir_all(&dir);
}
