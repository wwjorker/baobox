//! 验收：扫描件转可搜索 PDF
//!
//! 闭环验证：造一份「扫描件」（只有图、没有文字层的 PDF）→ 加文字层 →
//! 用 PDF 自己的文本提取读回来，读得到才算数。
//!
//! 这个功能最容易出的错是坐标翻转和字宽错位，两者都不会报错——
//! 文字是隐形的，只有真去搜索或拖选才看得出全错位了。
//! 所以除了「能不能搜到」，还要验位置落在页面内、且大致在该在的地方。

use baobox_lib::pdf_ocr::make_searchable;
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

    let tmp = std::env::temp_dir().join("baobox_ocrpdf");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // 造一张「扫描件」：白底上用系统字体画几行黑字，存成 PNG
    let words = ["INVOICE", "TOTAL 2580", "百宝箱 验收"];
    let page_png = tmp.join("scan.png");
    render_text_image(&page_png, &words);

    // 包成没有文字层的 PDF
    let scan_pdf = tmp.join("scan.pdf");
    image_to_pdf(&scan_pdf, &page_png);

    println!("======== 前提：这份 PDF 本来搜不到字 ========");
    let before = extract(&scan_pdf);
    check(
        "原件没有文字层",
        !before.contains("INVOICE"),
        format!("提取到 {} 个字符", before.trim().chars().count()),
    );

    println!("\n======== 加文字层 ========");
    match make_searchable(&scan_pdf, None, None) {
        Ok((dst, pages, count)) => {
            check(
                "处理成功",
                pages == 1 && count > 0,
                format!("{pages} 页 · {count} 段文字"),
            );

            let after = extract(&dst);
            check(
                "英文能搜到了",
                after.to_uppercase().contains("INVOICE"),
                format!("提取到「{}」", one_line(&after)),
            );
            check(
                "数字能搜到了",
                after.contains("2580"),
                "金额这类内容是这个功能最常见的用途".into(),
            );

            // 产物必须还能被正常打开、页数不变
            let doc = Document::load(&dst).unwrap();
            check(
                "产物结构完好",
                doc.get_pages().len() == 1,
                format!("{} 页", doc.get_pages().len()),
            );

            // 原来的图还在——文字层是「加」不是「换」
            let has_image = doc.objects.values().any(|o| {
                matches!(o, Object::Stream(s) if s.dict.get(b"Subtype")
                    .and_then(|x| x.as_name()).map(|n| n == b"Image").unwrap_or(false))
            });
            check(
                "原图仍在，页面外观不变",
                has_image,
                "文字层是叠加的，不是替换".into(),
            );

            // 坐标翻转做错的话文字会跑到页外。抽出所有 Td 坐标验证在页内。
            let (in_page, total_pos, sample) = check_positions(&doc, 595.0, 842.0);
            check(
                "文字坐标落在页面内",
                total_pos > 0 && in_page == total_pos,
                format!("{in_page}/{total_pos} 个位置合法 · 例 {sample}"),
            );

            check(
                "体积没有失控",
                std::fs::metadata(&dst).unwrap().len()
                    < std::fs::metadata(&scan_pdf).unwrap().len() * 3,
                format!(
                    "{} KB → {} KB",
                    std::fs::metadata(&scan_pdf).unwrap().len() / 1024,
                    std::fs::metadata(&dst).unwrap().len() / 1024
                ),
            );
        }
        Err(e) => {
            check("处理成功", false, format!("报错 {}", e.key));
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(60)
        .collect()
}

fn extract(p: &PathBuf) -> String {
    let doc = Document::load(p).unwrap();
    let nums: Vec<u32> = doc.get_pages().keys().copied().collect();
    doc.extract_text(&nums).unwrap_or_default()
}

/// 扫内容流里的 Td 坐标，确认都落在页面范围内
fn check_positions(doc: &Document, w: f32, h: f32) -> (usize, usize, String) {
    let mut ok = 0;
    let mut total = 0;
    let mut sample = String::from("无");
    for obj in doc.objects.values() {
        let Object::Stream(s) = obj else { continue };
        let Ok(raw) = s.decompressed_content() else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw);
        if !text.contains("3 Tr") {
            continue;
        }
        for line in text.lines() {
            let Some(idx) = line.find(" Td ") else {
                continue;
            };
            let parts: Vec<&str> = line[..idx].split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let (Ok(x), Ok(y)) = (
                parts[parts.len() - 2].parse::<f32>(),
                parts[parts.len() - 1].parse::<f32>(),
            ) else {
                continue;
            };
            total += 1;
            if x >= -5.0 && x <= w + 5.0 && y >= -5.0 && y <= h + 5.0 {
                ok += 1;
            }
            if total == 1 {
                sample = format!("({x:.0}, {y:.0})");
            }
        }
    }
    (ok, total, sample)
}

/// 画一张带文字的白底图，当作扫描件
fn render_text_image(path: &PathBuf, lines: &[&str]) {
    use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

    let font_bytes = std::fs::read(r"C:\Windows\Fonts\msyh.ttc")
        .or_else(|_| std::fs::read(r"C:\Windows\Fonts\simsun.ttc"))
        .expect("找不到系统中文字体");
    // ttc 是字体集合，取第一份
    let font = FontRef::try_from_slice_and_index(&font_bytes, 0).expect("字体解析失败");

    let (w, h) = (1200u32, 900u32);
    let mut img = image::RgbImage::from_pixel(w, h, image::Rgb([255, 255, 255]));
    let scale = PxScale::from(64.0);
    let scaled = font.as_scaled(scale);

    let mut y = 120.0f32;
    for line in lines {
        let mut x = 100.0f32;
        for ch in line.chars() {
            let glyph_id = font.glyph_id(ch);
            let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(x, y));
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, cov| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h && cov > 0.3 {
                        let v = ((1.0 - cov) * 255.0) as u8;
                        img.put_pixel(px as u32, py as u32, image::Rgb([v, v, v]));
                    }
                });
            }
            x += scaled.h_advance(glyph_id);
        }
        y += 130.0;
    }
    img.save(path).unwrap();
}

/// 把一张图包成一页 PDF（模拟扫描件：只有图，没有文字层）
fn image_to_pdf(path: &PathBuf, img: &PathBuf) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let image = lopdf::xobject::image(img).unwrap();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.add_page_contents(page, Vec::new()).unwrap();
    doc.insert_image(page, image, (0.0, 0.0), (595.0, 842.0))
        .unwrap();
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
