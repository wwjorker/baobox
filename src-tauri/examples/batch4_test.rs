//! 验收：第四批（元数据清除、裁边、ICO、分卷、按行处理）
//!
//! 分割与合并是闭环验证——切开再拼回来，字节必须跟原文件一模一样。
//! 差一个字节就是数据损坏，而拼出来的文件照样打得开，不比对发现不了。

use baobox_lib::image_edit::ico_file;
use baobox_lib::pdf_ops::{clean_metadata, crop_file};
use baobox_lib::textfile::{join_file, process_lines, split_file};
use lopdf::{dictionary, Document, Object};
use std::path::PathBuf;

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {name:<26} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {name:<26} {detail}");
        }
    };

    let tmp = std::env::temp_dir().join("baobox_batch4");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // ------------------------------------------------------ 元数据清除
    println!("======== PDF 元数据清除 ========");

    let meta_pdf = tmp.join("meta.pdf");
    make_pdf_with_meta(&meta_pdf);

    let before = read_info(&meta_pdf);
    check(
        "样本确实带着身份信息",
        before.contains("wty") && before.contains("Microsoft Word"),
        format!("原件 /Info: {}", before),
    );

    let (cleaned, removed) = clean_metadata(&meta_pdf, false).unwrap();
    let after = read_info(&cleaned);
    check(
        "作者等字段已清空",
        !after.contains("wty") && !after.contains("Microsoft Word"),
        format!("剩下 {}", if after.is_empty() { "（空）".into() } else { after.clone() }),
    );
    check(
        "如实报告清掉了什么",
        removed.contains(&"Author".to_string()) && removed.contains(&"Producer".to_string()),
        format!("{removed:?}"),
    );
    check(
        "XMP 那份独立副本也清了",
        removed.contains(&"XMP".to_string())
            && !has_xmp(&cleaned),
        "只清 /Info 的话属性面板干净了，XML 还在里面".into(),
    );

    let (kept, _) = clean_metadata(&meta_pdf, true).unwrap();
    check(
        "开关能保住时间字段",
        read_info(&kept).contains("D:2024"),
        "有些场景需要留存创建时间".into(),
    );

    // 产物还得能打开
    check(
        "产物结构完好",
        Document::load(&cleaned).map(|d| d.get_pages().len()).unwrap_or(0) == 1,
        "1 页".into(),
    );

    // ------------------------------------------------------ 裁边
    println!("\n======== PDF 裁掉页边空白 ========");

    // 一张四周大白边、中间一小块黑的图，包成 PDF
    let wide_margin = tmp.join("margin.png");
    image::RgbImage::from_fn(1000, 1400, |x, y| {
        if (400..600).contains(&x) && (600..800).contains(&y) {
            image::Rgb([0, 0, 0])
        } else {
            image::Rgb([255, 255, 255])
        }
    })
    .save(&wide_margin)
    .unwrap();
    let margin_pdf = tmp.join("margin.pdf");
    image_to_pdf(&margin_pdf, &wide_margin);

    match crop_file(&margin_pdf, 0.0) {
        Ok((dst, total, n)) => {
            check("裁了这一页", total == 1 && n == 1, format!("{n}/{total} 页"));
            let doc = Document::load(&dst).unwrap();
            let (_, page_id) = doc.get_pages().into_iter().next().unwrap();
            let cb = doc
                .get_object(page_id)
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"CropBox")
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| {
                    a.iter()
                        .filter_map(|o| match o {
                            Object::Real(f) => Some(*f),
                            Object::Integer(i) => Some(*i as f32),
                            _ => None,
                        })
                        .collect::<Vec<f32>>()
                })
                .unwrap_or_default();
            check("写入了 CropBox", cb.len() == 4, format!("{cb:?}"));
            // 内容占原图 20%×14%，裁后区域应该明显小于整页
            let shrunk = cb.len() == 4 && (cb[2] - cb[0]) < 595.0 * 0.5;
            check(
                "裁后区域明显变小",
                shrunk,
                format!("宽 {:.0} pt（原 595）", if cb.len() == 4 { cb[2] - cb[0] } else { 0.0 }),
            );
            check(
                "没有删掉页面内容",
                doc.objects.values().any(|o| matches!(o, Object::Stream(s) if s.dict
                    .get(b"Subtype").and_then(|x| x.as_name()).map(|n| n == b"Image").unwrap_or(false))),
                "只改显示区域，裁错了能还原".into(),
            );
        }
        Err(e) => check("裁了这一页", false, format!("报错 {}", e.key)),
    }

    // ------------------------------------------------------ ICO
    println!("\n======== 生成 ICO ========");

    let big = tmp.join("logo.png");
    image::RgbImage::from_fn(512, 400, |x, y| {
        image::Rgb([(x / 2) as u8, (y / 2) as u8, 200])
    })
    .save(&big)
    .unwrap();

    let (ico, n) = ico_file(&big).unwrap();
    let data = std::fs::read(&ico).unwrap();
    check("装入多种尺寸", n == 6, format!("{n} 种"));
    check(
        "文件头是合法 ICO",
        data[0..2] == [0, 0] && data[2..4] == [1, 0],
        format!("类型字段 = {}", u16::from_le_bytes([data[2], data[3]])),
    );
    check(
        "目录项数量对得上",
        u16::from_le_bytes([data[4], data[5]]) as usize == n,
        format!("头里写着 {} 个", u16::from_le_bytes([data[4], data[5]])),
    );
    // 256 那一项在宽高字节里必须写 0（字段只有 8 位）
    let last_entry = 6 + 16 * (n - 1);
    check(
        "256 尺寸按规范写 0",
        data[last_entry] == 0 && data[last_entry + 1] == 0,
        "8 位字段装不下 256".into(),
    );
    // 每个载荷都应该是能解开的 PNG
    let mut all_png = true;
    for i in 0..n {
        let e = 6 + 16 * i;
        let size = u32::from_le_bytes([data[e + 8], data[e + 9], data[e + 10], data[e + 11]]) as usize;
        let off = u32::from_le_bytes([data[e + 12], data[e + 13], data[e + 14], data[e + 15]]) as usize;
        if image::load_from_memory(&data[off..off + size]).is_err() {
            all_png = false;
        }
    }
    check("每个尺寸都是有效图像", all_png, format!("{n} 个载荷全部可解码"));

    // ------------------------------------------------------ 分卷闭环
    println!("\n======== 分割与合并（闭环）========");

    let bigfile = tmp.join("payload.bin");
    // 用可预测但不重复的字节，拼错顺序能被发现
    let content: Vec<u8> = (0..5_000_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&bigfile, &content).unwrap();

    let (_, parts) = split_file(&bigfile, 2).unwrap();
    check("切成了多卷", parts == 3, format!("5 MB ÷ 2 MB → {parts} 卷"));

    let out_dir = tmp.join("Baobox_output");
    let first = out_dir.join("payload.bin.001");
    check("第一卷命名正确", first.exists(), "payload.bin.001".into());

    let (joined, jn, total) = join_file(&first).unwrap();
    check("拼回了全部卷", jn == parts, format!("{jn} 卷"));
    check(
        "字节数一致",
        total as usize == content.len(),
        format!("{} vs {}", total, content.len()),
    );
    // 这条才是真验证：内容逐字节相同
    let back = std::fs::read(&joined).unwrap();
    check(
        "内容与原文件完全一致",
        back == content,
        "差一个字节就是数据损坏，而拼出来照样打得开".into(),
    );

    check(
        "不接受从中间那卷开始",
        join_file(&out_dir.join("payload.bin.002")).is_err(),
        "err.notFirstPart".into(),
    );
    let small = tmp.join("small.bin");
    std::fs::write(&small, b"tiny").unwrap();
    check(
        "比一卷还小时明确拒绝",
        split_file(&small, 20).is_err(),
        "err.smallerThanPart".into(),
    );

    // ------------------------------------------------------ 按行处理
    println!("\n======== 按行去重排序 ========");

    let lines = tmp.join("list.txt");
    std::fs::write(&lines, "banana\napple\nbanana\ncherry\napple\nbanana\n".as_bytes()).unwrap();

    let (d1, b1, a1) = process_lines(&lines, true, false, false).unwrap();
    let s1 = read_utf8(&d1);
    check(
        "去重",
        b1 == 6 && a1 == 3 && s1.lines().count() == 3,
        format!("{b1} 行 → {a1} 行"),
    );
    check("去重保持原顺序", s1.starts_with("banana"), one(&s1));

    let (d2, _, _) = process_lines(&lines, true, true, false).unwrap();
    let s2 = read_utf8(&d2);
    check("排序", s2.starts_with("apple"), one(&s2));

    let (d3, _, _) = process_lines(&lines, false, false, true).unwrap();
    let s3 = read_utf8(&d3);
    check(
        "词频按次数降序",
        s3.starts_with("3\tbanana"),
        one(&s3),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn one(s: &str) -> String {
    s.lines().collect::<Vec<_>>().join(" / ").chars().take(50).collect()
}

fn read_utf8(p: &PathBuf) -> String {
    let b = std::fs::read(p).unwrap();
    let body = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { &b[3..] } else { &b[..] };
    String::from_utf8_lossy(body).to_string()
}

fn read_info(p: &PathBuf) -> String {
    let doc = Document::load(p).unwrap();
    let Ok(Object::Reference(id)) = doc.trailer.get(b"Info").cloned() else {
        return String::new();
    };
    let Ok(d) = doc.get_object(id).and_then(|o| o.as_dict()) else {
        return String::new();
    };
    d.iter()
        .filter_map(|(k, v)| match v {
            Object::String(s, _) => Some(format!(
                "{}={}",
                String::from_utf8_lossy(k),
                String::from_utf8_lossy(s)
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_xmp(p: &PathBuf) -> bool {
    let doc = Document::load(p).unwrap();
    let Ok(Object::Reference(root)) = doc.trailer.get(b"Root").cloned() else {
        return false;
    };
    doc.get_object(root)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .map(|d| d.get(b"Metadata").is_ok())
        .unwrap_or(false)
}

fn make_pdf_with_meta(path: &PathBuf) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.add_page_contents(page, b"BT ET".to_vec()).unwrap();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)],
        }),
    );

    // 一份 XMP：跟 /Info 各存各的，清理必须两边都动
    let xmp = doc.add_object(Object::Stream(lopdf::Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        br#"<?xpacket?><x:xmpmeta><dc:creator>wty</dc:creator></x:xmpmeta>"#.to_vec(),
    )));
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "Metadata" => xmp,
    });
    doc.trailer.set("Root", catalog);

    let info = doc.add_object(dictionary! {
        "Title" => Object::string_literal("季度报告"),
        "Author" => Object::string_literal("wty"),
        "Creator" => Object::string_literal("Microsoft Word"),
        "Producer" => Object::string_literal("Microsoft Word"),
        "CreationDate" => Object::string_literal("D:20240301120000"),
    });
    doc.trailer.set("Info", info);
    doc.save(path).unwrap();
}

fn image_to_pdf(path: &PathBuf, img: &PathBuf) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let image = lopdf::xobject::image(img).unwrap();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.add_page_contents(page, Vec::new()).unwrap();
    doc.insert_image(page, image, (0.0, 0.0), (595.0, 842.0)).unwrap();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)],
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}
