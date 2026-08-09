//! 后端安全回归 —— 真正跑进 `cargo test` 的断言。
//!
//! `examples/` 下那批 `*_test.rs` 是手动 `cargo run --example` 的验收程序，
//! 打印通过/失败计数、且多数依赖 make_samples 生成的大样本，`cargo test`
//! 根本不碰它们。这里放的是自包含、失败即 panic 的 `#[test]`：fixture 全部
//! 代码生成，不依赖外部样本，任何机器都能跑，专门盯住几条最要紧的不变量和
//! 近期改动的回归。

use baobox_lib::redact::{redact_image, RedactMode, Region};

/// 安全红线 7：打码必须真的改写选区内的像素，且一个像素都不能碰到选区外。
///
/// 网页上那种「盖一层黑矩形」的假打码，原始数据还躺在文件里。这里在图上写一段
/// 高频花纹当敏感内容，打码后逐像素核对它是否已被销毁、区外是否毫发无伤。
#[test]
fn redact_destroys_pixels_in_region_only() {
    let (w, h) = (400u32, 200u32);
    let mut before = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            // 左半边高频花纹（有信息可销毁），右半边纯色对照
            let p = if x < w / 2 {
                let v = (((x * 7 + y * 13) % 2) * 255) as u8;
                image::Rgba([v, 255 - v, (x % 251) as u8, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            };
            before.put_pixel(x, y, p);
        }
    }
    let regions = [Region { x: 0.0, y: 0.0, w: 0.5, h: 1.0 }];

    for (name, mode) in [("blackout", RedactMode::Blackout), ("pixelate", RedactMode::Pixelate)] {
        let mut work = before.clone();
        redact_image(&mut work, &regions, mode);

        // 选区内：原始像素几乎不该有幸存者
        let mut survivors = 0usize;
        for y in 0..h {
            for x in 0..w / 2 {
                if work.get_pixel(x, y) == before.get_pixel(x, y) {
                    survivors += 1;
                }
            }
        }
        let total = (h * w / 2) as usize;
        assert!(
            survivors * 100 / total < 5,
            "{name}: 选区内仍有 {survivors}/{total} 个原像素残留，打码没真正销毁内容",
        );

        // 选区外：一个像素都不能变
        for y in 0..h {
            for x in w / 2..w {
                assert_eq!(
                    work.get_pixel(x, y),
                    before.get_pixel(x, y),
                    "{name}: 选区外的像素被误伤了",
                );
            }
        }
    }
}

/// 本轮修的损坏 PDF 回归：merge_docs 曾因 max_id 未同步，让新建的 /Pages、
/// /Catalog 覆盖掉已导入的内容/字体对象——产出的 PDF 首页字体指向 Catalog、
/// 提取文本时 panic。这里合并两份各带独特内容流的单页 PDF，断言结果是 2 页、
/// 且两段内容流都还在（没被覆盖）。
#[test]
fn merge_keeps_both_documents_content() {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream, StringFormat};

    fn one_page(marker: &str) -> Document {
        let mut doc = Document::with_version("1.5");
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::String(marker.as_bytes().to_vec(), StringFormat::Literal)],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let page_id = doc.new_object_id();
        let pages_id = doc.new_object_id();
        doc.objects.insert(
            page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        );
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    let merged = baobox_lib::pdf_ops::merge_docs(vec![one_page("PAGE-AAA"), one_page("PAGE-BBB")])
        .expect("合并应成功");

    let pages = merged.get_pages();
    assert_eq!(pages.len(), 2, "合并后应当是 2 页");

    let mut blob = Vec::new();
    for (_, pid) in &pages {
        blob.extend(merged.get_page_content(*pid).expect("取页内容"));
    }
    let text = String::from_utf8_lossy(&blob);
    assert!(
        text.contains("PAGE-AAA") && text.contains("PAGE-BBB"),
        "两页的内容流都应保留（原 bug 会让新建的 /Pages、/Catalog 覆盖掉它们）",
    );
}
