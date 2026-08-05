//! 验收：第六批（解压、GIF、画布扩展、建文件夹、PDF 修复）
//!
//! 解压那几条是重点：GBK 名的 zip 要手工造，因为 zip crate 写出来的
//! 一律是 UTF-8，用它造不出「会坏」的样本。目录穿越同理，必须手工造。

use baobox_lib::archive::{create_zip, extract};
use baobox_lib::image_edit::{expand_file, gif_frames, make_gif};
use baobox_lib::pdf_ops::repair_file;
use baobox_lib::textfile::make_dirs;
use lopdf::{dictionary, Document, Object};
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {name:<28} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {name:<28} {detail}");
        }
    };

    let tmp = std::env::temp_dir().join("baobox_batch6");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("Baobox_output");

    // ==================================================== 解压
    println!("======== 解压（中文名乱码修复）========");

    // 「报告.txt」和「资料/说明.txt」的 GBK 字节，不置 UTF-8 标志位
    let gbk_zip = tmp.join("gbk名.zip");
    write_zip_raw(
        &gbk_zip,
        &[
            (
                vec![0xB1, 0xA8, 0xB8, 0xE6, b'.', b't', b'x', b't'], // 报告.txt
                b"content one".to_vec(),
            ),
            (
                {
                    // 资料/说明.txt
                    let mut v = vec![0xD7, 0xCA, 0xC1, 0xCF, b'/'];
                    v.extend_from_slice(&[0xCB, 0xB5, 0xC3, 0xF7]);
                    v.extend_from_slice(b".txt");
                    v
                },
                b"content two".to_vec(),
            ),
        ],
    );

    match extract(&gbk_zip, None) {
        Ok(rep) => {
            check("解出两个文件", rep.files == 2, format!("{} 个", rep.files));
            check(
                "认出名字需要修复",
                rep.fixed_names == 2,
                format!("{} 个名字修过", rep.fixed_names),
            );
            let base = out.join("gbk名");
            check(
                "中文名解对了",
                base.join("报告.txt").exists(),
                "报告.txt".into(),
            );
            check(
                "子目录里的也对",
                base.join("资料").join("说明.txt").exists(),
                "资料/说明.txt".into(),
            );
            check(
                "内容没串",
                std::fs::read_to_string(base.join("报告.txt")).unwrap_or_default() == "content one",
                "第一个文件内容对得上".into(),
            );
            check(
                "自动建了同名文件夹",
                base.is_dir(),
                "不是把几十个文件倒进同一层".into(),
            );
        }
        Err(e) => check("解出两个文件", false, format!("报错 {}", e.key)),
    }

    // 目录穿越：这一条必须被拦掉
    let evil = tmp.join("恶意.zip");
    write_zip_raw(
        &evil,
        &[
            (b"../../escaped.txt".to_vec(), b"should not land".to_vec()),
            (b"ok.txt".to_vec(), b"fine".to_vec()),
        ],
    );
    match extract(&evil, None) {
        Ok(rep) => {
            check(
                "拦掉目录穿越",
                rep.rejected == 1 && rep.files == 1,
                format!("拦 {} 条，解 {} 条", rep.rejected, rep.files),
            );
            check(
                "没有文件落到上级目录",
                !tmp.parent().unwrap().join("escaped.txt").exists()
                    && !tmp.join("escaped.txt").exists(),
                "解压一个恶意包不该能往外写".into(),
            );
        }
        Err(e) => check("拦掉目录穿越", false, format!("报错 {}", e.key)),
    }

    let notzip = tmp.join("假的.zip");
    std::fs::write(&notzip, b"this is not a zip").unwrap();
    check(
        "不是 zip 时明确报错",
        extract(&notzip, None).is_err(),
        "err.badArchive".into(),
    );

    // ==================================================== 打包成 zip（闭环）
    println!("\n======== 压缩成 ZIP（打包→解回，闭环）========");

    // 造一个带子目录的源，文件名故意用中文，才能验「打包不留乱码」
    let src_root = tmp.join("打包源");
    std::fs::create_dir_all(src_root.join("子目录")).unwrap();
    std::fs::write(src_root.join("甲.txt"), "alpha 内容一").unwrap();
    std::fs::write(src_root.join("子目录").join("乙.txt"), "beta 内容二").unwrap();

    // 拖整个文件夹进来：文件夹名和层次都该原样进 zip
    match create_zip(&[src_root.clone()]) {
        Ok((zip_path, n)) => {
            check("打包 2 个文件", n == 2, format!("{n} 个"));
            check(
                "产物是 zip 且名字带「打包」",
                zip_path.extension().and_then(|e| e.to_str()) == Some("zip")
                    && zip_path.file_stem().map(|s| s.to_string_lossy().contains("打包")).unwrap_or(false)
                    && zip_path.is_file(),
                zip_path.file_name().unwrap().to_string_lossy().into_owned(),
            );

            // 闭环：拿自己的解压把它解回来，逐项比对
            match extract(&zip_path, None) {
                Ok(rep) => {
                    check("解回 2 个文件", rep.files == 2, format!("{} 个", rep.files));
                    let top = rep.dir.join("打包源");
                    check(
                        "文件夹名和子目录层次都保住了",
                        top.join("甲.txt").is_file()
                            && top.join("子目录").join("乙.txt").is_file(),
                        "打包源/甲.txt、打包源/子目录/乙.txt".into(),
                    );
                    check(
                        "解回时无需修名字（证明写的是 UTF-8）",
                        rep.fixed_names == 0,
                        format!("{} 个名字被判为需修", rep.fixed_names),
                    );
                    let a_ok = std::fs::read_to_string(top.join("甲.txt")).unwrap_or_default()
                        == "alpha 内容一";
                    let b_ok = std::fs::read_to_string(top.join("子目录").join("乙.txt"))
                        .unwrap_or_default()
                        == "beta 内容二";
                    check("内容逐字节一致（甲）", a_ok, "打包再解回没串内容".into());
                    check("内容逐字节一致（乙）", b_ok, "子目录里的也对得上".into());
                }
                Err(e) => check("解回 2 个文件", false, format!("解压报错 {}", e.key)),
            }
        }
        Err(e) => check("打包 2 个文件", false, format!("打包报错 {}", e.key)),
    }

    // 单分支目录不该被压扁——旧的「取共同祖先」实现在这里会把
    // 文件夹名和中间层全丢掉，只剩光文件名。Codex 指出的洞，专门守一条。
    let deep = tmp.join("只有一枝");
    std::fs::create_dir_all(deep.join("里层")).unwrap();
    std::fs::write(deep.join("里层").join("独苗.txt"), "solo").unwrap();
    match create_zip(&[deep.clone()]) {
        Ok((zip_path, _)) => match extract(&zip_path, None) {
            Ok(rep) => check(
                "单分支目录不压扁",
                rep.dir.join("只有一枝").join("里层").join("独苗.txt").is_file(),
                "文件夹名和中间层都在".into(),
            ),
            Err(e) => check("单分支目录不压扁", false, format!("解压报错 {}", e.key)),
        },
        Err(e) => check("单分支目录不压扁", false, format!("打包报错 {}", e.key)),
    }

    // 拖同一层的散文件：条目就是干净的文件名，不带路径
    let loose_a = tmp.join("散一.txt");
    let loose_b = tmp.join("散二.txt");
    std::fs::write(&loose_a, "l1").unwrap();
    std::fs::write(&loose_b, "l2").unwrap();
    match create_zip(&[loose_a.clone(), loose_b.clone()]) {
        Ok((zip_path, n)) => {
            check("散文件打包 2 个", n == 2, format!("{n} 个"));
            match extract(&zip_path, None) {
                Ok(rep) => check(
                    "散文件是干净文件名",
                    rep.dir.join("散一.txt").is_file() && rep.dir.join("散二.txt").is_file(),
                    "顶层两个文件，无多余目录".into(),
                ),
                Err(e) => check("散文件是干净文件名", false, format!("解压报错 {}", e.key)),
            }
        }
        Err(e) => check("散文件打包 2 个", false, format!("打包报错 {}", e.key)),
    }

    // 全是空文件夹 / 没有真文件时，明确报错而不是产出空包
    let empty_dir = tmp.join("空壳");
    std::fs::create_dir_all(&empty_dir).unwrap();
    check(
        "没有可打包的文件时报错",
        matches!(create_zip(&[empty_dir.clone()]), Err(e) if e.key == "err.zipNoFiles"),
        "err.zipNoFiles".into(),
    );

    // ==================================================== 画布扩展
    println!("\n======== 画布扩展 ========");

    let wide = tmp.join("wide.png");
    image::RgbImage::from_pixel(1600, 900, image::Rgb([200, 60, 60]))
        .save(&wide)
        .unwrap();

    let (edst, nw, nh) = expand_file(&wide, "1:1", false).unwrap();
    check(
        "补成正方形",
        nw == 1600 && nh == 1600,
        format!("1600×900 → {nw}×{nh}"),
    );
    let e = image::open(&edst).unwrap().to_rgba8();
    check(
        "原画面一个像素没切",
        e.get_pixel(800, 800).0[..3] == [200, 60, 60],
        "中心仍是原内容".into(),
    );
    check(
        "补出来的边是白的",
        e.get_pixel(800, 20).0[..3] == [255, 255, 255],
        "顶部是填充色".into(),
    );

    let (_, dw, dh) = expand_file(&wide, "16:9", false).unwrap();
    check(
        "本来就是这比例时不动",
        dw == 1600 && dh == 900,
        format!("{dw}×{dh}"),
    );

    // ==================================================== GIF
    println!("\n======== GIF 制作与拆帧 ========");

    // 三张不同颜色、其中一张尺寸不同，用来验「统一到第一张尺寸」
    let mut frames = Vec::new();
    for (i, (c, w, h)) in [
        ([220u8, 40, 40], 200u32, 150u32),
        ([40, 200, 40], 200, 150),
        ([40, 40, 220], 320, 240),
    ]
    .iter()
    .enumerate()
    {
        let p = tmp.join(format!("f{}.png", i + 1));
        image::RgbImage::from_pixel(*w, *h, image::Rgb(*c)).save(&p).unwrap();
        frames.push(p);
    }

    let (gif, n) = make_gif(&frames, 200).unwrap();
    check("做出 3 帧动图", n == 3, format!("{n} 帧"));
    check(
        "产物是能解的 GIF",
        image::open(&gif).is_ok(),
        format!("{} KB", std::fs::metadata(&gif).unwrap().len() / 1024),
    );

    let (_, total, saved) = gif_frames(&gif, 1).unwrap();
    check(
        "拆回 3 帧",
        total == 3 && saved == 3,
        format!("共 {total} 帧，导出 {saved} 张"),
    );

    // 尺寸不同的第三张应该被统一到 200×150。
    // 注意产物在 out/Baobox_output 里——gif 本身就在 out 里，
    // 而输出目录一律建在输入文件旁边，所以嵌了一层。
    let f3 = out
        .join("Baobox_output")
        .join(format!("{}_帧003.png", gif.file_stem().unwrap().to_string_lossy()));
    if f3.exists() {
        let i3 = image::open(&f3).unwrap();
        check(
            "所有帧统一到第一张尺寸",
            i3.width() == 200 && i3.height() == 150,
            format!("第三帧 {}×{}（原 320×240）", i3.width(), i3.height()),
        );
    } else {
        check("所有帧统一到第一张尺寸", false, "找不到第三帧产物".into());
    }

    let (_, _, every2) = gif_frames(&gif, 2).unwrap();
    check("隔帧导出", every2 == 2, format!("每隔 2 帧 → {every2} 张"));
    check(
        "单张拒绝做动图",
        make_gif(&frames[..1], 200).is_err(),
        "err.gifNeedTwo".into(),
    );

    // ==================================================== 建文件夹
    println!("\n======== 按清单建文件夹 ========");

    let listdir = tmp.join("建目录");
    std::fs::create_dir_all(&listdir).unwrap();
    let list = listdir.join("清单.txt");
    std::fs::write(
        &list,
        "一班\r\n二班\r\n2026/01\r\n../危险\r\n一班\r\n非法:名字\r\n".as_bytes(),
    )
    .unwrap();

    let (base, made, skipped) = make_dirs(&list).unwrap();
    check(
        "建在清单旁边",
        base == listdir,
        "就地组织，不是丢进输出目录".into(),
    );
    check("建了 4 个", made == 4, format!("建 {made} · 跳 {skipped}"));
    check("普通名字", listdir.join("一班").is_dir(), "一班".into());
    check(
        "嵌套写法",
        listdir.join("2026").join("01").is_dir(),
        "2026/01".into(),
    );
    check(
        "拒绝往上级建",
        !listdir.parent().unwrap().join("危险").exists(),
        "../危险 被跳过".into(),
    );
    check(
        "非法字符换成下划线",
        listdir.join("非法_名字").is_dir(),
        "非法:名字 → 非法_名字".into(),
    );
    check("重复的跳过", skipped == 2, format!("跳了 {skipped} 个"));

    // ==================================================== PDF 修复
    println!("\n======== 修复损坏的 PDF ========");

    // 造一份 xref 偏移被打乱的 PDF：内容完好，索引失效
    let good = tmp.join("good.pdf");
    make_pdf(&good, 3);
    let broken = tmp.join("坏索引.pdf");
    let mut bytes = std::fs::read(&good).unwrap();
    // 把 startxref 后面的数字改成一个错的偏移
    if let Some(pos) = find_last(&bytes, b"startxref") {
        let num_start = pos + 9;
        for i in num_start..bytes.len() {
            if bytes[i].is_ascii_digit() {
                bytes[i] = b'9';
            } else if bytes[i] == b'%' {
                break;
            }
        }
    }
    std::fs::write(&broken, &bytes).unwrap();

    match repair_file(&broken) {
        Ok((dst, pages, raster)) => {
            check(
                "救回来了",
                pages == 3,
                format!("{pages} 页 · {}", if raster { "降级为图片" } else { "原样重建" }),
            );
            check(
                "产物能正常打开",
                Document::load(&dst).map(|d| d.get_pages().len()).unwrap_or(0) == 3,
                "重新读回来 3 页".into(),
            );
            if !raster {
                // 没降级的话文字层应该还在
                let d = Document::load(&dst).unwrap();
                let nums: Vec<u32> = d.get_pages().keys().copied().collect();
                let t = d.extract_text(&nums).unwrap_or_default();
                check(
                    "文字层没丢",
                    t.contains("Page 1"),
                    "首选路径不该丢文字".into(),
                );
            }
        }
        Err(e) => check("救回来了", false, format!("报错 {}", e.key)),
    }

    let garbage = tmp.join("彻底坏.pdf");
    std::fs::write(&garbage, b"%PDF-1.5\nnot really a pdf at all\n").unwrap();
    check(
        "真的救不了时明确报错",
        repair_file(&garbage).is_err(),
        "不产出一份空文件冒充成功".into(),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn find_last(hay: &[u8], needle: &[u8]) -> Option<usize> {
    (0..hay.len().saturating_sub(needle.len()))
        .rev()
        .find(|&i| &hay[i..i + needle.len()] == needle)
}

/// 手工写一个 zip：条目名用原始字节，且**不置** UTF-8 标志位。
///
/// zip crate 写出来的名字一律是 UTF-8，用它造不出「会坏」的样本——
/// 而这个功能整个要解决的就是那种样本。
fn write_zip_raw(path: &Path, entries: &[(Vec<u8>, Vec<u8>)]) {
    let mut f = std::fs::File::create(path).unwrap();
    let mut offsets = Vec::new();
    let mut pos = 0u32;

    for (name, data) in entries {
        let crc = crc32(data);
        offsets.push((pos, crc));
        // 本地文件头
        f.write_all(&0x0403_4b50u32.to_le_bytes()).unwrap();
        f.write_all(&20u16.to_le_bytes()).unwrap(); // 版本
        f.write_all(&0u16.to_le_bytes()).unwrap(); // 标志位：第 11 位留 0，即「不是 UTF-8」
        f.write_all(&0u16.to_le_bytes()).unwrap(); // 方法 0 = 不压缩
        f.write_all(&0u16.to_le_bytes()).unwrap(); // 时间
        f.write_all(&0u16.to_le_bytes()).unwrap(); // 日期
        f.write_all(&crc.to_le_bytes()).unwrap();
        f.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&(name.len() as u16).to_le_bytes()).unwrap();
        f.write_all(&0u16.to_le_bytes()).unwrap(); // 扩展字段长度
        f.write_all(name).unwrap();
        f.write_all(data).unwrap();
        pos += 30 + name.len() as u32 + data.len() as u32;
    }

    let cd_start = pos;
    for (i, (name, data)) in entries.iter().enumerate() {
        let (off, crc) = offsets[i];
        f.write_all(&0x0201_4b50u32.to_le_bytes()).unwrap();
        f.write_all(&20u16.to_le_bytes()).unwrap(); // 制作版本
        f.write_all(&20u16.to_le_bytes()).unwrap(); // 需要版本
        f.write_all(&0u16.to_le_bytes()).unwrap(); // 标志位，同样留 0
        f.write_all(&0u16.to_le_bytes()).unwrap();
        f.write_all(&0u16.to_le_bytes()).unwrap();
        f.write_all(&0u16.to_le_bytes()).unwrap();
        f.write_all(&crc.to_le_bytes()).unwrap();
        f.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&(name.len() as u16).to_le_bytes()).unwrap();
        f.write_all(&0u16.to_le_bytes()).unwrap();
        f.write_all(&0u16.to_le_bytes()).unwrap();
        f.write_all(&0u16.to_le_bytes()).unwrap();
        f.write_all(&0u16.to_le_bytes()).unwrap();
        f.write_all(&0u32.to_le_bytes()).unwrap();
        f.write_all(&off.to_le_bytes()).unwrap();
        f.write_all(name).unwrap();
        pos += 46 + name.len() as u32;
    }

    let cd_size = pos - cd_start;
    f.write_all(&0x0605_4b50u32.to_le_bytes()).unwrap();
    f.write_all(&0u16.to_le_bytes()).unwrap();
    f.write_all(&0u16.to_le_bytes()).unwrap();
    f.write_all(&(entries.len() as u16).to_le_bytes()).unwrap();
    f.write_all(&(entries.len() as u16).to_le_bytes()).unwrap();
    f.write_all(&cd_size.to_le_bytes()).unwrap();
    f.write_all(&cd_start.to_le_bytes()).unwrap();
    f.write_all(&0u16.to_le_bytes()).unwrap();
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn make_pdf(path: &PathBuf, n: u32) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for i in 1..=n {
        let c = format!("BT /F1 36 Tf 100 600 Td (Page {i}) Tj ET");
        let cid = doc.add_object(lopdf::Stream::new(dictionary! {}, c.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Count" => n, "Kids" => kids }),
    );
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.save(path).unwrap();
}
