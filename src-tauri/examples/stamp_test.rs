//! 中文水印闭环验收（方案风险 2 的最终检验）
//!
//! 光看「产物能打开」不够——字体子集化的全部意义在于**中文得真的显示出来**，
//! 而且要在没装这个字体的机器上也显示。所以这里走一条完整回路：
//!
//!   加水印 → 用系统引擎渲染成图 → OCR 读回文字 → 比对是不是原文
//!
//! OCR 能从渲染结果里认出那几个字，就说明字形确实被嵌进去并画出来了。

use baobox_lib::pdf_font;
use baobox_lib::pdf_ops::{stamp_file, StampOptions};
use baobox_lib::pdf_render::render_page;
use lopdf::{Document, Object};
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| {
        r"C:\Users\wty\AppData\Local\Temp\claude\f--AI--\aba2743b-ab4b-4ea7-a33a-b47ab2ed99fa\scratchpad\pdftest".into()
    }));
    let sandbox = std::env::temp_dir().join("baobox_stamp_test");
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(&sandbox).unwrap();

    let src0 = std::fs::read_dir(&dir)
        .expect("测试目录不存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().map(|e| e == "pdf").unwrap_or(false))
        .expect("没有可用的测试 PDF");
    let src = sandbox.join(src0.file_name().unwrap());
    std::fs::copy(&src0, &src).unwrap();

    let watermark = "百宝箱机密";
    println!("======== 中文水印闭环验收 ========\n");

    // ---- 1. 子集化本身 ----
    match pdf_font::prepare(&format!("{watermark}第页共 0123456789")) {
        Ok(f) => println!(
            "  [1] 子集化      原字体 {:.1} MB → 子集 {:.1} KB（1/{:.0}），覆盖 {} 个字形",
            f.source_bytes as f64 / 1e6,
            f.data.len() as f64 / 1024.0,
            f.source_bytes as f64 / f.data.len() as f64,
            f.gid_of.len()
        ),
        Err(e) => {
            println!("  [1] 子集化      失败 {}", e.key);
            return;
        }
    }

    // ---- 2. 加水印 + 页码 ----
    let opt = StampOptions {
        watermark: watermark.into(),
        page_numbers: true,
        opacity: 0.25,
    };
    let (dst, pages, _) = match stamp_file(&src, &opt) {
        Ok(v) => v,
        Err(e) => {
            println!("  [2] 加标记      失败 {}", e.key);
            return;
        }
    };
    println!("  [2] 加标记      {pages} 页处理完成");

    // ---- 3. 产物结构完好 + 字体确实嵌进去了 ----
    match Document::load(&dst) {
        Ok(doc) => {
            let np = doc.get_pages().len();
            let embedded = doc
                .objects
                .values()
                .any(|o| matches!(o, Object::Stream(s) if s.dict.get(b"Length1").is_ok()));
            println!(
                "  [3] 产物结构    {} 页（原 {pages} 页），字体已嵌入: {}",
                np, embedded
            );
            if np != pages || !embedded {
                println!("      !! 结构不符，中止");
                return;
            }
        }
        Err(e) => {
            println!("  [3] 产物结构    打不开: {e}");
            return;
        }
    }

    // ---- 4. 渲染 + OCR 读回 ----
    let png = match render_page(&dst, 0, 1600) {
        Ok(v) => v,
        Err(e) => {
            println!("  [4] 渲染        失败 {}", e.key);
            return;
        }
    };
    let img_path = sandbox.join("rendered.png");
    std::fs::write(&img_path, &png).unwrap();
    println!(
        "  [4] 渲染        首页 → PNG {:.0} KB",
        png.len() as f64 / 1024.0
    );

    // 页码是水平实色的，OCR 读得了；水印是 45 度旋转 + 25% 透明的，
    // OCR 读不了——那不是缺陷，恰恰是水印该有的样子。两者走的是同一套
    // 字体嵌入和绘制代码，所以页码被读回来就证明了整条链路通。
    match baobox_lib::ocr::recognize_for_test(&img_path) {
        Ok(text) => {
            let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            let hit_page = flat.contains("第1页") && flat.contains("共3页");
            println!("  [5] OCR 读回    页码命中: {hit_page}");
            println!("      识别到: {}", text.replace('\n', " / "));

            // 水印是旋转+半透明的，OCR 读不了；而按字形编号去内容流里比对
            // 也不可靠——单独子集化会得到另一套编号。所以直接看渲染结果：
            // 加过水印的页面，着墨的像素必然明显变多。
            let ink = |png: &[u8]| -> f64 {
                let Ok(img) = image::load_from_memory(png) else {
                    return 0.0;
                };
                let g = img.to_luma8();
                let dark = g.pixels().filter(|p| p.0[0] < 240).count();
                dark as f64 / g.pixels().len() as f64 * 100.0
            };
            let before = render_page(&src, 0, 1600).map(|b| ink(&b)).unwrap_or(0.0);
            let after = ink(&png);
            let more = after > before * 1.5;
            println!(
                "  [6] 着墨对比    原页 {before:.3}% → 加标记后 {after:.3}%（明显增加: {more}）"
            );

            println!(
                "\n======== {} ========",
                if hit_page && more {
                    "通过：中文字形嵌入、渲染、绘制三步全部成立"
                } else {
                    "失败"
                }
            );
        }
        Err(e) => println!("  [5] OCR 读回    失败 {}", e.key),
    }
}
