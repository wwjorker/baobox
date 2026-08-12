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
    let regions = [Region {
        x: 0.0,
        y: 0.0,
        w: 0.5,
        h: 1.0,
    }];

    for (name, mode) in [
        ("blackout", RedactMode::Blackout),
        ("pixelate", RedactMode::Pixelate),
    ] {
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
                    vec![Object::String(
                        marker.as_bytes().to_vec(),
                        StringFormat::Literal,
                    )],
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

/// 安全红线 7（PDF 侧）：清元数据必须把 XMP 那份独立 XML 真正从产物里删掉，
/// 不能只从 Catalog 摘掉引用、把流对象留在文件里。造一份挂着 XMP 流的 PDF，
/// 清完后在「产物原始字节」和「重载后的对象表」两头都确认那段 XML 不见了。
#[test]
fn clean_metadata_actually_removes_xmp() {
    use lopdf::{dictionary, Document, Object, Stream};

    const MARK: &str = "x:xmpmeta-BAOBOX-PROBE";

    let mut doc = Document::with_version("1.5");
    let xmp = format!("<?xpacket begin='' id=''?><x:xmpmeta>{MARK}</x:xmpmeta><?xpacket end='w'?>");
    let xmp_id = doc.add_object(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        xmp.into_bytes(),
    ));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
    });
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
        "Metadata" => xmp_id,
    });
    doc.trailer.set("Root", catalog_id);

    let tmp = std::env::temp_dir().join("baobox_xmp_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let src = tmp.join("has_xmp.pdf");
    doc.save(&src).unwrap();

    let has_mark = |bytes: &[u8]| bytes.windows(MARK.len()).any(|w| w == MARK.as_bytes());
    assert!(
        has_mark(&std::fs::read(&src).unwrap()),
        "fixture 本应带 XMP marker"
    );

    let (dst, removed) = baobox_lib::pdf_ops::clean_metadata(&src, false).expect("清元数据应成功");
    assert!(removed.iter().any(|r| r == "XMP"), "应报告清掉了 XMP");

    // 产物的原始字节里不该再搜得到那段 XML（不是只断引用、把流留在文件里）
    assert!(
        !has_mark(&std::fs::read(&dst).unwrap()),
        "清元数据后产物字节里仍有 XMP——那份 XML 没被真正删掉（红线 7）",
    );

    // 重载后也不该再有 Metadata 流对象残留
    let reloaded = Document::load(&dst).unwrap();
    let has_meta_stream = reloaded.objects.values().any(|o| {
        o.as_stream()
            .ok()
            .and_then(|s| s.dict.get(b"Type").ok())
            .and_then(|t| t.as_name().ok())
            == Some(&b"Metadata"[..])
    });
    assert!(!has_meta_stream, "清元数据后仍残留 Metadata 流对象");

    let _ = std::fs::remove_dir_all(&tmp);
}

/// ZIP 解压零成功时，本次建的输出目录要整棵清掉、不留空壳（Codex 复审）。
#[test]
fn zip_extract_cleans_up_when_nothing_extracted() {
    use zip::write::SimpleFileOptions;

    let tmp = std::env::temp_dir().join("baobox_zip_cleanup");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // 只有一个目录条目、没有任何文件——解压一个文件都不会成功
    let zip_path = tmp.join("only_dirs.zip");
    {
        let f = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        w.add_directory("emptydir/", SimpleFileOptions::default())
            .unwrap();
        w.finish().unwrap();
    }

    let res = baobox_lib::archive::extract(&zip_path, None);
    assert!(res.is_err(), "只有目录的包应判为空、返回错误");

    // 本次建的 Baobox_output/only_dirs 应被整棵删掉
    let out = tmp.join("Baobox_output").join("only_dirs");
    assert!(!out.exists(), "零成功解压后不该残留空的输出目录");

    let _ = std::fs::remove_dir_all(&tmp);
}

/// ZIP 累计上限回归（Codex 复审建议补的自动测试）：三条各 1 MiB 的条目、累计上限
/// 设 1.5 MiB，应只解出第一条、其余因跨累计线被拒——用参数化的小限额替代「真写满
/// 8 GiB」。它盯住「累计上限确实生效、且随实际写出的字节累加」：谁把它改成不限、
/// 或只按条目数算，都会解出多于 1 个文件而挂掉。
///
/// 诚实说明：诚实 zip 的声明尺寸=实际，所以这条测不了「deflate 谎报 uncompressed_size」
/// 那种绕过——那需手工构造恶意包，已由 Codex 手工验证（新代码按实际 take 截断、不看
/// 声明尺寸），此处不重复造。
#[test]
fn zip_cumulative_limit_counts_actual_bytes() {
    use baobox_lib::archive::Limits;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let tmp = std::env::temp_dir().join("baobox_zip_cumulative");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let zip_path = tmp.join("three.zip");
    {
        let f = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        // Stored（不压缩）：声明尺寸=实际字节，测的正是「累计按实际写出的字节算」
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for i in 0..3 {
            w.start_file(format!("f{i}.bin"), opts).unwrap();
            w.write_all(&vec![0u8; 1 << 20]).unwrap(); // 1 MiB
        }
        w.finish().unwrap();
    }

    let limits = Limits {
        max_entry: 4u64 << 20, // 单条 4 MiB，不卡单条
        max_total: 3u64 << 19, // 累计 1.5 MiB
        max_entries: 100,
    };
    let rep = baobox_lib::archive::extract_with_limits(&zip_path, None, &limits)
        .expect("第一条应成功，函数返回 Ok");
    assert_eq!(rep.files, 1, "累计 1.5 MiB 下只该解出第 1 个 1 MiB 文件");
    assert!(rep.rejected >= 1, "跨累计线的后续条目应被拒");

    let _ = std::fs::remove_dir_all(&tmp);
}
