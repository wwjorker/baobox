//! 图片水印验收
//!
//! 和 PDF 水印一样走闭环：打上中文水印 → OCR 读回来 → 比对原文。
//! 能读出来才证明字形是真被光栅化到像素上了，而不是画了一堆空白。

use baobox_lib::watermark::watermark_file;

fn main() {
    let dir = std::env::temp_dir().join("baobox_wm_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 浅色底，方便 OCR 认深色水印
    let mut img = image::RgbImage::new(1400, 900);
    for p in img.pixels_mut() {
        *p = image::Rgb([246, 246, 244]);
    }
    let src = dir.join("blank.png");
    image::DynamicImage::ImageRgb8(img).save(&src).unwrap();

    let text = "仅供内部使用";
    println!("======== 图片水印验收 ========\n");

    let mut pass = 0;
    let mut fail = 0;
    let mut check = |l: &str, ok: bool, d: String| {
        if ok { pass += 1; println!("  [OK]   {l:<16} {d}") } else { fail += 1; println!("  [FAIL] {l:<16} {d}") }
    };

    // ---- 平铺模式 ----
    match watermark_file(&src, text, 0.55, true) {
        Ok((dst, n)) => {
            check("平铺绘制", n > 4, format!("绘制 {n} 处"));
            match baobox_lib::ocr::recognize_for_test(&dst) {
                Ok(got) => {
                    let flat: String = got.chars().filter(|c| !c.is_whitespace()).collect();
                    check(
                        "OCR 读回",
                        flat.contains(text),
                        format!("识别到 {} 个字符，含原文: {}", flat.chars().count(), flat.contains(text)),
                    );
                }
                Err(e) => check("OCR 读回", false, e.key),
            }
        }
        Err(e) => check("平铺绘制", false, e.key),
    }

    // ---- 单个模式：只应画一处 ----
    match watermark_file(&src, text, 0.6, false) {
        Ok((_, n)) => check("单处绘制", n == 1, format!("绘制 {n} 处")),
        Err(e) => check("单处绘制", false, e.key),
    }

    // ---- 原图必须没被动过 ----
    let orig = std::fs::read(&src).unwrap();
    let again = image::load_from_memory(&orig).unwrap().to_rgb8();
    let untouched = again.pixels().all(|p| p.0 == [246, 246, 244]);
    check("原图未被修改", untouched, format!("{untouched}"));

    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    let _ = std::fs::remove_dir_all(&dir);
}
