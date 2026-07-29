//! 生成一套试用素材，覆盖全部新增工具。
//!
//! 每个样本都是为某个具体工具「特意造成会暴露问题的样子」——
//! 比例参差的截图用来验拼接对齐，一简对多繁的词用来验转换表，
//! 引号里带逗号的 CSV 用来验解析。拿干净规整的样本测不出东西。
//!
//! 用法：cargo run --release --example make_samples

use lopdf::{dictionary, Document, Object};
use std::path::{Path, PathBuf};

fn main() {
    let root = PathBuf::from(r"F:\dev\百宝箱试用素材");
    // 只清我们自己生成的那几个子目录，用户放在别处的东西不动
    for sub in ["图片", "文本", "PDF", "大文件", "时间戳测试"] {
        let d = root.join(sub);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
    }

    let img = root.join("图片");
    let txt = root.join("文本");
    let pdf = root.join("PDF");
    let big = root.join("大文件");
    let ts = root.join("时间戳测试");

    // ================================================== 图片
    // 16:9 彩色照片：验按比例裁切、九宫格、调色、主色调
    image::RgbImage::from_fn(1600, 900, |x, y| {
        let band = (y / 150) % 6;
        let base = [
            [220u32, 40, 40],
            [240, 150, 30],
            [230, 220, 40],
            [40, 180, 90],
            [40, 120, 220],
            [140, 60, 200],
        ][band as usize];
        // 加一点横向渐变，压缩和调色的效果才看得出来
        image::Rgb([
            (base[0] * (200 + x / 8) / 400).min(255) as u8,
            (base[1] * (200 + x / 8) / 400).min(255) as u8,
            (base[2] * (200 + x / 8) / 400).min(255) as u8,
        ])
    })
    .save(img.join("彩色横图_1600x900.jpg"))
    .unwrap();

    // 四周厚白边，中间有内容：验自动去白边
    image::RgbImage::from_fn(800, 600, |x, y| {
        if (250..550).contains(&x) && (200..400).contains(&y) {
            image::Rgb([(x % 200) as u8, 60, 180])
        } else {
            image::Rgb([255, 255, 255])
        }
    })
    .save(img.join("四周白边.png"))
    .unwrap();

    // 正方形：验生成 ICO、圆角边框
    image::RgbImage::from_fn(512, 512, |x, y| {
        let d = ((x as f32 - 256.0).powi(2) + (y as f32 - 256.0).powi(2)).sqrt();
        if d < 200.0 {
            image::Rgb([255, 60, 24])
        } else {
            image::Rgb([255, 228, 77])
        }
    })
    .save(img.join("方形图标源_512.png"))
    .unwrap();

    // 三张宽度不同的「聊天截图」：验长图拼接会不会对齐到最窄、顺序对不对
    for (i, w) in [(1u32, 720u32), (2, 640), (3, 800)] {
        image::RgbImage::from_fn(w, 300, |x, y| {
            let bubble = (y / 90) % 3;
            let inside = x > 40 && x < w - 40 && (y % 90) > 12 && (y % 90) < 78;
            if inside {
                match (bubble + i) % 3 {
                    0 => image::Rgb([210, 240, 210]),
                    1 => image::Rgb([215, 225, 250]),
                    _ => image::Rgb([250, 235, 205]),
                }
            } else {
                image::Rgb([245, 245, 245])
            }
        })
        .save(img.join(format!("聊天截图_{i}_宽{w}.png")))
        .unwrap();
    }

    // ================================================== 文本
    // 真正的 GBK 字节：验乱码修复
    std::fs::write(
        txt.join("这是GBK编码_直接打开会乱码.txt"),
        vec![
            0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4, 0xC4, 0xDA, 0xC8, 0xDD, 0x0D, 0x0A,
            0xB0, 0xD9, 0xB1, 0xA6, 0xCF, 0xE4, 0x0D, 0x0A, 0xC2, 0xD2, 0xC2, 0xEB, 0xD0, 0xDE,
            0xB8, 0xB4, 0xD1, 0xE9, 0xCA, 0xD5,
        ],
    )
    .unwrap();

    // 一简对多繁的词全在这儿：验简繁转换是按词还是逐字
    std::fs::write(
        txt.join("简繁转换_难例.txt"),
        "头发很长，发展很快。\r\n\
         干净的干部把树干弄干了。\r\n\
         里面有三里路。\r\n\
         这只表表示时间。\r\n"
            .as_bytes(),
    )
    .unwrap();

    // 一行一条，含中文和空行：验二维码批量生成
    std::fs::write(
        txt.join("二维码内容_一行一条.txt"),
        "https://github.com/wwjorker/baobox\r\n\
         百宝箱 中文内容测试\r\n\
         \r\n\
         ASSET-2026-00427\r\n"
            .as_bytes(),
    )
    .unwrap();

    // 有重复有乱序：验去重、排序、词频
    std::fs::write(
        txt.join("名单_有重复.txt"),
        "张伟\r\n李娜\r\n张伟\r\n王芳\r\n李娜\r\n张伟\r\n陈明\r\n王芳\r\n".as_bytes(),
    )
    .unwrap();

    // 引号里含逗号、双写引号、中文列名：验 CSV 解析
    let mut csv = vec![0xEF, 0xBB, 0xBF];
    csv.extend_from_slice(
        "姓名,备注,金额\r\n\
         张三,\"含,逗号的备注\",1200\r\n\
         李四,\"带\"\"引号\"\"的备注\",860\r\n\
         王五,普通备注,430\r\n"
            .as_bytes(),
    );
    std::fs::write(txt.join("表格_含特殊字符.csv"), csv).unwrap();

    // JSON 数组：验反向转 CSV（第三条故意缺一个字段）
    std::fs::write(
        txt.join("数据_转CSV用.json"),
        r#"[
  {"产品": "键盘", "价格": 299, "标签": "外设,输入"},
  {"产品": "显示器", "价格": 1499, "标签": "显示"},
  {"产品": "鼠标", "价格": 159}
]"#
        .as_bytes(),
    )
    .unwrap();

    std::fs::write(
        txt.join("查找替换_测试.txt"),
        "订单 A-1001 已发货\r\n订单 A-1002 待处理\r\n订单 B-2001 已取消\r\nHello world, hello WORLD\r\n"
            .as_bytes(),
    )
    .unwrap();

    // ================================================== PDF
    make_numbered_pdf(&pdf.join("六页_带页码.pdf"), 6);
    make_meta_pdf(&pdf.join("带作者信息_清元数据用.pdf"));
    make_wide_margin_pdf(&pdf.join("超宽白边_裁边用.pdf"));
    make_image_pdf(
        &pdf.join("内嵌图片_提取用.pdf"),
        &img.join("彩色横图_1600x900.jpg"),
    );
    make_scan_pdf(&pdf.join("模拟扫描件_转可搜索用.pdf"));

    // ================================================== 大文件
    let payload: Vec<u8> = (0..5_000_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(big.join("五兆测试文件.bin"), &payload).unwrap();

    // ================================================== 时间戳
    // 这个工具会直接改原文件，所以单独放，别跟别的样本混
    for i in 1..=3 {
        std::fs::write(ts.join(format!("会被改时间_{i}.txt")), b"content").unwrap();
    }

    // ================================================== 记录哈希
    let mut lines = Vec::new();
    walk(&root, &mut lines);
    lines.sort();
    std::fs::write(root.join("_原文件哈希.txt"), lines.join("\r\n").as_bytes()).unwrap();

    println!("素材已生成：{}", root.display());
    println!("共 {} 个文件，哈希清单见 _原文件哈希.txt\n", lines.len());
    for sub in ["图片", "文本", "PDF", "大文件", "时间戳测试"] {
        let n = std::fs::read_dir(root.join(sub)).map(|d| d.count()).unwrap_or(0);
        println!("  {sub:<10} {n} 个");
    }
}

fn walk(dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.file_name().map(|n| n != "_原文件哈希.txt").unwrap_or(true) {
            let data = std::fs::read(&p).unwrap_or_default();
            out.push(format!("{}  {}", blake3::hash(&data).to_hex(), p.display()));
        }
    }
}

fn make_numbered_pdf(path: &Path, n: u32) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for i in 1..=n {
        let c = format!(
            "BT /F1 48 Tf 200 500 Td (Page {i}) Tj ET\n\
             BT /F1 14 Tf 200 450 Td (baobox test document) Tj ET"
        );
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

fn make_meta_pdf(path: &Path) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let cid = doc.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /F1 18 Tf 60 700 Td (Check File > Properties before and after) Tj ET".to_vec(),
    ));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)],
        }),
    );
    let xmp = doc.add_object(Object::Stream(lopdf::Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description>
<dc:creator>wty</dc:creator><xmp:CreatorTool>Microsoft Word</xmp:CreatorTool>
</rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
            .to_vec(),
    )));
    let cat = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "Metadata" => xmp,
    });
    doc.trailer.set("Root", cat);
    let info = doc.add_object(dictionary! {
        "Title" => Object::string_literal("2026 第一季度报告"),
        "Author" => Object::string_literal("wty"),
        "Creator" => Object::string_literal("Microsoft Word 2021"),
        "Producer" => Object::string_literal("Microsoft Word 2021"),
        "Subject" => Object::string_literal("内部资料，请勿外传"),
        "CreationDate" => Object::string_literal("D:20260115093000+08'00'"),
    });
    doc.trailer.set("Info", info);
    doc.save(path).unwrap();
}

fn make_wide_margin_pdf(path: &Path) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for i in 1..=3 {
        // 内容只占中间一小块，四周全是空白
        let c = format!(
            "0 0 0 rg\n240 400 120 60 re f\n\
             BT /F1 11 Tf 236 380 Td (narrow content, page {i}) Tj ET"
        );
        let cid = doc.add_object(lopdf::Stream::new(dictionary! {}, c.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Count" => 3, "Kids" => kids }),
    );
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.save(path).unwrap();
}

fn make_image_pdf(path: &Path, src: &Path) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let image = lopdf::xobject::image(src).unwrap();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.add_page_contents(page, Vec::new()).unwrap();
    doc.insert_image(page, image, (20.0, 300.0), (555.0, 312.0))
        .unwrap();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)],
        }),
    );
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.save(path).unwrap();
}

/// 造一份「扫描件」：只有图，没有任何文字层。
/// 图上的字是用系统字体画出来的像素，所以处理前搜不到——正是要验的前提。
fn make_scan_pdf(path: &Path) {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

    let bytes = std::fs::read(r"C:\Windows\Fonts\msyh.ttc")
        .or_else(|_| std::fs::read(r"C:\Windows\Fonts\simsun.ttc"))
        .expect("找不到系统中文字体");
    let font = FontRef::try_from_slice_and_index(&bytes, 0).unwrap();

    let (w, h) = (1240u32, 1754u32); // A4 @150dpi
    let mut canvas = image::RgbImage::from_pixel(w, h, image::Rgb([252, 251, 248]));
    let lines = [
        ("INVOICE  发票", 72.0f32),
        ("编号 NO. 2026-00427", 44.0),
        ("客户 张三建筑设计有限公司", 44.0),
        ("合计金额 12580.00 元", 52.0),
        ("开票日期 2026-01-15", 44.0),
        ("备注：本页文字是画上去的像素，", 36.0),
        ("处理前搜不到，处理后应该能搜到。", 36.0),
    ];
    let mut y = 220.0f32;
    for (text, size) in lines {
        let scale = PxScale::from(size);
        let sf = font.as_scaled(scale);
        let mut x = 140.0f32;
        for ch in text.chars() {
            let gid = font.glyph_id(ch);
            let g = gid.with_scale_and_position(scale, ab_glyph::point(x, y));
            if let Some(o) = font.outline_glyph(g) {
                let bb = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let px = bb.min.x as i32 + gx as i32;
                    let py = bb.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h && cov > 0.25 {
                        let v = ((1.0 - cov) * 250.0) as u8;
                        canvas.put_pixel(px as u32, py as u32, image::Rgb([v, v, v]));
                    }
                });
            }
            x += sf.h_advance(gid);
        }
        y += size * 1.9;
    }

    let png = std::env::temp_dir().join("baobox_scan_src.png");
    canvas.save(&png).unwrap();

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let image = lopdf::xobject::image(&png).unwrap();
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
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.save(path).unwrap();
    let _ = std::fs::remove_file(&png);
}
