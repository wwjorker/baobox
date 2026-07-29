//! 边界与异常输入验收
//!
//! 前面那些测试走的都是正常路径。真正会在用户手里炸的是这些：
//! 空文件、扩展名骗人的文件、超长中文路径、带透明通道的图、
//! 损坏的 PDF。这里逐个喂进去，看它们是**干净地报错**还是崩掉、
//! 或者更糟——悄悄产出一个坏文件。

use std::path::PathBuf;

fn main() {
    let dir = std::env::temp_dir().join("baobox_edge_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut pass = 0;
    let mut fail = 0;
    let mut note = |ok: bool, label: &str, detail: String| {
        if ok { pass += 1; println!("  [OK]   {label:<26} {detail}") }
        else { fail += 1; println!("  [FAIL] {label:<26} {detail}") }
    };

    println!("======== 边界与异常输入 ========\n");

    // ---- 1. 0 字节文件 ----
    let empty = dir.join("empty.jpg");
    std::fs::write(&empty, b"").unwrap();
    let r = std::panic::catch_unwind(|| {
        image::open(&empty).map(|_| ()).map_err(|e| e.to_string())
    });
    note(
        matches!(r, Ok(Err(_))),
        "空文件",
        match &r {
            Ok(Err(e)) => format!("干净报错: {}", e.chars().take(40).collect::<String>()),
            Ok(Ok(_)) => "居然解析成功了？".into(),
            Err(_) => "!! panic".into(),
        },
    );

    // ---- 2. 扩展名骗人：文本文件改名成 .jpg ----
    let fake = dir.join("fake.jpg");
    std::fs::write(&fake, b"this is definitely not a jpeg, just plain text\n").unwrap();
    let r = std::panic::catch_unwind(|| image::open(&fake).map(|_| ()).map_err(|e| e.to_string()));
    note(
        matches!(r, Ok(Err(_))),
        "假扩展名",
        match &r {
            Ok(Err(_)) => "被识别为非图片，干净报错".into(),
            Ok(Ok(_)) => "!! 当成图片处理了".into(),
            Err(_) => "!! panic".into(),
        },
    );

    // ---- 3. 损坏的 PDF：只有文件头 ----
    let broken = dir.join("broken.pdf");
    std::fs::write(&broken, b"%PDF-1.7\n%%EOF").unwrap();
    let r = std::panic::catch_unwind(|| {
        lopdf::Document::load(&broken).map(|d| d.get_pages().len()).map_err(|e| e.to_string())
    });
    note(
        !matches!(r, Err(_)),
        "残缺 PDF",
        match &r {
            Ok(Err(e)) => format!("干净报错: {}", e.chars().take(40).collect::<String>()),
            Ok(Ok(n)) => format!("解析出 {n} 页（空文档）"),
            Err(_) => "!! panic".into(),
        },
    );

    // ---- 4. 超长中文路径（逼近 Windows 260 上限）----
    let mut deep = dir.clone();
    for _ in 0..12 {
        deep = deep.join("这是一个很长的中文目录名用来测试路径上限");
    }
    let deep_ok = std::fs::create_dir_all(baobox_lib::paths::long_path(&deep)).is_ok();
    let mut long_file_ok = false;
    if deep_ok {
        let f = deep.join("测试图片.png");
        let img = image::RgbImage::new(60, 60);
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut buf, image::ImageFormat::Png).unwrap();
        long_file_ok = std::fs::write(baobox_lib::paths::long_path(&f), buf.into_inner()).is_ok()
            && baobox_lib::paths::long_path(&f).exists();
    }
    note(
        deep_ok && long_file_ok,
        "超长中文路径",
        format!("{} 字符，建目录 {deep_ok}，写文件 {long_file_ok}", deep.to_string_lossy().chars().count()),
    );

    // ---- 5. 文件名含特殊字符 ----
    let odd = dir.join("图 片 (1) [副本] #2 &test.png");
    let img = image::RgbImage::new(80, 80);
    let mut b = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut b, image::ImageFormat::Png).unwrap();
    std::fs::write(&odd, b.into_inner()).unwrap();
    let wm = baobox_lib::watermark::watermark_file(&odd, "测试", 0.5, false);
    note(wm.is_ok(), "特殊字符文件名", match &wm {
        Ok((p, _)) => format!("产出 {}", p.file_name().unwrap().to_string_lossy()),
        Err(e) => format!("!! {}", e.key),
    });

    // ---- 6. 带透明通道的 PNG 压缩 ----
    let alpha = dir.join("alpha.png");
    let mut a = image::RgbaImage::new(200, 200);
    for (i, p) in a.pixels_mut().enumerate() {
        *p = image::Rgba([(i % 255) as u8, 80, 200, (i % 200) as u8]);
    }
    let mut ab = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(a).write_to(&mut ab, image::ImageFormat::Png).unwrap();
    std::fs::write(&alpha, ab.into_inner()).unwrap();
    let loaded = image::open(&alpha).unwrap();
    let r = baobox_lib::image_ops::compress_to_target(&loaded, baobox_lib::image_ops::OutFmt::WebP, 50_000);
    note(r.is_ok(), "带透明通道的图", match &r {
        Ok(t) => format!("压到 {} KB，质量 {}", t.bytes.len()/1024, t.quality),
        Err(e) => format!("!! {}", e.key),
    });

    // ---- 7. 只有 1 像素的图 ----
    let tiny = dir.join("tiny.png");
    let t1 = image::RgbImage::new(1, 1);
    let mut tb = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(t1).write_to(&mut tb, image::ImageFormat::Png).unwrap();
    std::fs::write(&tiny, tb.into_inner()).unwrap();
    let r = std::panic::catch_unwind(|| {
        let i = image::open(&tiny).unwrap();
        baobox_lib::image_ops::compress_to_target(&i, baobox_lib::image_ops::OutFmt::Jpeg, 1000).is_ok()
    });
    note(matches!(r, Ok(true)), "1×1 像素图", match &r {
        Ok(true) => "正常处理".into(),
        Ok(false) => "返回错误（可接受）".into(),
        Err(_) => "!! panic".into(),
    });

    // ---- 8. 重命名：空规则链不应改动任何东西 ----
    let pv = baobox_lib::rename::rename_preview(
        vec![odd.to_string_lossy().to_string()],
        vec![],
    );
    note(
        pv.iter().all(|p| p.unchanged),
        "重命名空规则",
        format!("{}/{} 标记为未变化", pv.iter().filter(|p| p.unchanged).count(), pv.len()),
    );

    // ---- 9. 水印文字为空 ----
    let r = baobox_lib::watermark::watermark_file(&odd, "   ", 0.5, false);
    note(r.is_err(), "空水印文字", match &r {
        Err(e) => format!("被拒绝: {}", e.key),
        Ok(_) => "!! 空文字也产出了文件".into(),
    });

    // ---- 10. 打码：选区超出图片边界 ----
    let mut img2 = image::RgbaImage::new(100, 100);
    for p in img2.pixels_mut() { *p = image::Rgba([10, 20, 30, 255]) }
    let r = std::panic::catch_unwind(move || {
        let mut i = img2;
        baobox_lib::redact::redact_image(
            &mut i,
            &[baobox_lib::redact::Region { x: 0.8, y: 0.8, w: 5.0, h: 5.0 }],
            baobox_lib::redact::RedactMode::Blackout,
        )
    });
    note(matches!(r, Ok(_)), "选区越界", match &r {
        Ok(n) => format!("处理 {n} 处，未越界崩溃"),
        Err(_) => "!! panic".into(),
    });

    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    let _ = std::fs::remove_dir_all(baobox_lib::paths::long_path(&dir));
}
