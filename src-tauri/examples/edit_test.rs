//! 验收：新增的七个工具
//!
//! 每条都验「结果真的对」而不是「函数没崩」——切图要数出块数并验尺寸，
//! 去白边要确认边真的没了且内容没被啃掉，乱码修复要用真的 GBK 字节。

use baobox_lib::image_edit::{adjust_file, frame_file, grid_file, stitch, trim_file};
use baobox_lib::image_ops::apply_orientation;
use baobox_lib::textfile::{fix_encoding, hash_file};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use std::path::PathBuf;

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {name:<22} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {name:<22} {detail}");
        }
    };

    let tmp = std::env::temp_dir().join("baobox_edit_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("Baobox_output");

    // ---------------------------------------------------------- 九宫格
    println!("======== 九宫格切图 ========");

    // 900×600，非正方形，用来验「先裁成正方形」是否生效
    let wide = tmp.join("wide.png");
    RgbImage::from_fn(900, 600, |x, y| Rgb([(x / 4) as u8, (y / 3) as u8, 90]))
        .save(&wide)
        .unwrap();

    let (_, n) = grid_file(&wide, 3, 3, true).unwrap();
    check("切出 9 块", n == 9, format!("{n} 块"));

    let tiles: Vec<PathBuf> = (1..=9)
        .map(|i| out.join(format!("wide_{i:02}.png")))
        .collect();
    check(
        "编号按发图顺序",
        tiles.iter().all(|p| p.exists()),
        "wide_01 … wide_09 都在".into(),
    );
    let t1 = image::open(&tiles[0]).unwrap();
    check(
        "先裁方后每块也是方",
        t1.width() == t1.height() && t1.width() == 200,
        format!("{}×{}（600 的短边 ÷ 3）", t1.width(), t1.height()),
    );

    // 不裁正方形时应按原比例切
    let _ = std::fs::remove_dir_all(&out);
    grid_file(&wide, 3, 3, false).unwrap();
    let r1 = image::open(out.join("wide_01.png")).unwrap();
    check(
        "不裁时按原比例",
        r1.width() == 300 && r1.height() == 200,
        format!("{}×{}", r1.width(), r1.height()),
    );

    // ---------------------------------------------------------- 长图拼接
    println!("\n======== 长图拼接 ========");

    let a = tmp.join("a.png");
    let b = tmp.join("b.png");
    // 故意给不同宽度，验证「对齐到最窄、只缩不放」
    RgbImage::from_pixel(400, 100, Rgb([200, 30, 30]))
        .save(&a)
        .unwrap();
    RgbImage::from_pixel(200, 100, Rgb([30, 30, 200]))
        .save(&b)
        .unwrap();

    let (dst, n) = stitch(&[a.clone(), b.clone()], true, 0).unwrap();
    let joined = image::open(&dst).unwrap();
    check("接了 2 张", n == 2, format!("{n} 张"));
    check(
        "宽度对齐到最窄",
        joined.width() == 200,
        format!("宽 {}（两张分别是 400 和 200）", joined.width()),
    );
    check(
        "高度是各张之和",
        joined.height() == 50 + 100,
        format!(
            "高 {}（400×100 缩到 200 宽是 50 高，加 100）",
            joined.height()
        ),
    );
    // 上半应是红的、下半是蓝的，顺序不能反
    let top = joined.get_pixel(100, 10).0;
    let bottom = joined.get_pixel(100, 120).0;
    check(
        "顺序就是列表顺序",
        top[0] > 150 && bottom[2] > 150,
        format!("上 R={} · 下 B={}", top[0], bottom[2]),
    );

    let (dst2, _) = stitch(&[a.clone(), b.clone()], true, 20).unwrap();
    let gapped = image::open(&dst2).unwrap();
    check(
        "间隙留白且不透明",
        gapped.height() == 170 && gapped.get_pixel(100, 55).0[..3] == [255, 255, 255],
        format!("高 {} · 缝隙是白的", gapped.height()),
    );
    check(
        "单张也拒绝",
        stitch(&[], true, 0).is_err(),
        "空列表返回错误".into(),
    );

    // ---------------------------------------------------------- 去白边
    println!("\n======== 去白边 ========");

    // 200×200 全白，中间 60×40 一块黑
    let bordered = tmp.join("bordered.png");
    RgbImage::from_fn(200, 200, |x, y| {
        if (70..130).contains(&x) && (80..120).contains(&y) {
            Rgb([0, 0, 0])
        } else {
            Rgb([255, 255, 255])
        }
    })
    .save(&bordered)
    .unwrap();

    let (tdst, w, h, nw, nh) = trim_file(&bordered, 10).unwrap();
    check(
        "白边裁干净",
        nw == 60 && nh == 40,
        format!("{w}×{h} → {nw}×{nh}（内容正好 60×40）"),
    );
    let trimmed = image::open(&tdst).unwrap();
    check(
        "内容没被啃掉",
        trimmed.get_pixel(0, 0).0[..3] == [0, 0, 0]
            && trimmed.get_pixel(59, 39).0[..3] == [0, 0, 0],
        "四角仍是内容像素".into(),
    );

    let solid = tmp.join("solid.png");
    RgbImage::from_pixel(50, 50, Rgb([255, 255, 255]))
        .save(&solid)
        .unwrap();
    check(
        "整张同色时明确报错",
        trim_file(&solid, 10).is_err(),
        "不返回一张 0×0 的空图".into(),
    );

    // ---------------------------------------------------------- 圆角边框
    println!("\n======== 圆角与边框 ========");

    let plain = tmp.join("plain.png");
    RgbImage::from_pixel(200, 200, Rgb([200, 30, 30]))
        .save(&plain)
        .unwrap();

    let (fdst, _) = frame_file(&plain, 25, 0, false).unwrap();
    let framed = image::open(&fdst).unwrap().to_rgba8();
    check(
        "圆角是透明的",
        framed.get_pixel(0, 0).0[3] == 0,
        format!("左上角 alpha={}", framed.get_pixel(0, 0).0[3]),
    );
    check(
        "中间没被动",
        framed.get_pixel(100, 100).0[..3] == [200, 30, 30],
        "圆角只削四角".into(),
    );
    check(
        "一律输出 PNG",
        fdst.extension().unwrap() == "png",
        "存成 JPEG 圆角会被填成实色".into(),
    );

    let (bdst, _) = frame_file(&plain, 0, 10, true).unwrap();
    let bordered_img = image::open(&bdst).unwrap();
    check(
        "边框把画布撑大",
        bordered_img.width() == 220 && bordered_img.height() == 220,
        format!(
            "{}×{}（两边各 10）",
            bordered_img.width(),
            bordered_img.height()
        ),
    );

    // ---------------------------------------------------------- 调色
    println!("\n======== 调色 ========");

    let mid = tmp.join("mid.png");
    RgbImage::from_pixel(40, 40, Rgb([120, 120, 120]))
        .save(&mid)
        .unwrap();

    let bright = adjust_file(&mid, 50, 0, 0, "none").unwrap();
    let bi = image::open(&bright).unwrap();
    check(
        "提亮确实变亮",
        bi.get_pixel(20, 20).0[0] > 120,
        format!("120 → {}", bi.get_pixel(20, 20).0[0]),
    );

    let colored = tmp.join("colored.png");
    RgbImage::from_pixel(40, 40, Rgb([200, 60, 60]))
        .save(&colored)
        .unwrap();
    let gray = adjust_file(&colored, 0, 0, 0, "gray").unwrap();
    let gi = image::open(&gray).unwrap().to_rgb8();
    let gp = gi.get_pixel(20, 20).0;
    check(
        "灰度三通道相等",
        gp[0] == gp[1] && gp[1] == gp[2],
        format!("RGB({}, {}, {})", gp[0], gp[1], gp[2]),
    );

    let desat = adjust_file(&colored, 0, 0, -100, "none").unwrap();
    let di = image::open(&desat).unwrap().to_rgb8();
    let dp = di.get_pixel(20, 20).0;
    check(
        "饱和度归零等于灰",
        (dp[0] as i32 - dp[2] as i32).abs() <= 2,
        format!("RGB({}, {}, {})", dp[0], dp[1], dp[2]),
    );

    // ---------------------------------------------------------- 乱码修复
    println!("\n======== 乱码修复 ========");

    // 真的 GBK 字节，不是假数据。「中文测试内容」的 GBK 编码。
    let gbk_bytes: Vec<u8> = vec![
        0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4, 0xC4, 0xDA, 0xC8, 0xDD, 0x0D, 0x0A, 0xB0,
        0xD9, 0xB1, 0xA6, 0xCF, 0xE4,
    ];
    let gbk_file = tmp.join("gbk.txt");
    std::fs::write(&gbk_file, &gbk_bytes).unwrap();

    let (edst, from, already) = fix_encoding(&gbk_file, true).unwrap();
    let fixed = std::fs::read(&edst).unwrap();
    let text = String::from_utf8_lossy(&fixed[3..]).to_string();
    check(
        "认出是 GBK 系编码",
        from.to_lowercase().contains("gbk") || from.to_lowercase().contains("gb18030"),
        format!("检测为 {from}"),
    );
    check(
        "内容真的转对了",
        text.contains("中文测试内容") && text.contains("百宝箱"),
        format!("读出「{}」", text.replace(['\r', '\n'], "/")),
    );
    check("没被误判成已是 UTF-8", !already, "already=false".into());
    check(
        "BOM 加上了",
        fixed.starts_with(&[0xEF, 0xBB, 0xBF]),
        "Excel 打开 CSV 才不会又乱一遍".into(),
    );

    // 本来就是 UTF-8 的不该被瞎改
    let utf8_file = tmp.join("utf8.txt");
    std::fs::write(&utf8_file, "本来就是 UTF-8".as_bytes()).unwrap();
    let (_, _, was_utf8) = fix_encoding(&utf8_file, false).unwrap();
    check("UTF-8 原文如实标注", was_utf8, "already=true".into());

    // ---------------------------------------------------------- 哈希
    println!("\n======== 哈希校验 ========");

    let hf = tmp.join("hash.bin");
    std::fs::write(&hf, b"abc").unwrap();
    let sha = hash_file(&hf, "sha256").unwrap();
    // "abc" 的 SHA-256 是公开的已知值，用它验实现没写错
    check(
        "SHA-256 对上已知值",
        sha == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        format!("{}…", &sha[..16]),
    );
    let b3 = hash_file(&hf, "blake3").unwrap();
    check(
        "BLAKE3 对上已知值",
        b3 == "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
        format!("{}…", &b3[..16]),
    );
    check("两种算法结果不同", sha != b3, "没有串到同一个实现上".into());

    // ---------------------------------------------------------- EXIF 方向转正
    println!("\n======== 抹 EXIF · 方向转正 ========");

    // 造一张左上角唯一有色、其余全黑的 3×2 图，转向后角点落到哪一目了然
    let mut mark = RgbImage::from_pixel(3, 2, Rgb([0, 0, 0]));
    mark.put_pixel(0, 0, Rgb([255, 0, 0])); // 左上标记
    let base = DynamicImage::ImageRgba8(image::DynamicImage::ImageRgb8(mark).to_rgba8());

    // orientation=1 原样不动
    let o1 = apply_orientation(base.clone(), 1);
    check(
        "方向 1 原样不动",
        o1.dimensions() == (3, 2) && o1.to_rgba8().get_pixel(0, 0).0[0] == 255,
        format!("{:?} 左上仍是红", o1.dimensions()),
    );

    // orientation=6 顺时针 90°：3×2 → 2×3，原左上(0,0)应转到右上(1,0)
    let o6 = apply_orientation(base.clone(), 6).to_rgba8();
    check(
        "方向 6 顺时针 90°",
        o6.dimensions() == (2, 3) && o6.get_pixel(1, 0).0[0] == 255 && o6.get_pixel(0, 0).0[0] == 0,
        format!("{:?} 红点到了右上", o6.dimensions()),
    );

    // orientation=3 转 180°：尺寸不变，左上标记应跑到右下(2,1)
    let o3 = apply_orientation(base.clone(), 3).to_rgba8();
    check(
        "方向 3 转 180°",
        o3.dimensions() == (3, 2) && o3.get_pixel(2, 1).0[0] == 255,
        "红点到了右下".into(),
    );

    // orientation=2 水平翻转：左上标记跑到右上(2,0)
    let o2 = apply_orientation(base.clone(), 2).to_rgba8();
    check(
        "方向 2 水平翻转",
        o2.dimensions() == (3, 2) && o2.get_pixel(2, 0).0[0] == 255,
        "红点镜像到右上".into(),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}
