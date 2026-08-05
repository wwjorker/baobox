//! 验收：PDF 页面整理（可视化选页/重排/逐页旋转的后端 arrange_pages）。
//!
//! 每页给一个不同的 MediaBox 宽度当「记号」，重排后按宽度就能核出顺序对不对——
//! 不用往页面里塞文字，也能验「保留哪些页、什么次序、各转多少」都落实了。

use baobox_lib::pdf_ops::{arrange_pages, PageOp};
use lopdf::{dictionary, Document, Object};
use std::path::Path;

fn make_pdf(path: &Path, widths: &[i64]) {
    make_pdf_rot(path, widths, &vec![0i64; widths.len()]);
}

/// 每页给定宽度和初始 Rotate。页面带一个空内容流（不然合并管线会把它当无效页丢掉）。
fn make_pdf_rot(path: &Path, widths: &[i64], rots: &[i64]) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::new();
    for (i, &w) in widths.iter().enumerate() {
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), w.into(), 800.into()],
            "Resources" => dictionary! {},
        };
        if rots[i] != 0 {
            page.set("Rotate", rots[i]);
        }
        let page_id = doc.add_object(page);
        doc.add_page_contents(page_id, Vec::new()).unwrap();
        kids.push(page_id.into());
    }
    let pages = dictionary! {
        "Type" => "Pages",
        "Count" => widths.len() as i64,
        "Kids" => kids,
    };
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

fn num(o: &Object) -> i64 {
    match o {
        Object::Integer(i) => *i,
        Object::Real(f) => *f as i64,
        _ => 0,
    }
}

/// 读出输出 PDF 每页的 (MediaBox 宽度, Rotate)，按页序排列。
fn read_pages(path: &Path) -> Vec<(i64, i64)> {
    let doc = Document::load(path).unwrap();
    let mut out = Vec::new();
    for (_, id) in doc.get_pages() {
        let dict = doc.get_object(id).unwrap().as_dict().unwrap();
        let w = dict
            .get(b"MediaBox")
            .ok()
            .and_then(|o| o.as_array().ok())
            .map(|a| num(&a[2]))
            .unwrap_or(0);
        let rot = dict.get(b"Rotate").ok().map(num).unwrap_or(0);
        out.push((w, rot));
    }
    out
}

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {name:<24} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {name:<24} {detail}");
        }
    };

    let tmp = std::env::temp_dir().join("baobox_arrange");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    println!("======== PDF 页面整理（选 / 排 / 转）========");

    // 4 页，宽度 110/120/130/140 当记号（= 原第 1/2/3/4 页）
    let src = tmp.join("四页.pdf");
    make_pdf(&src, &[110, 120, 130, 140]);

    // 保留 3、1、4 页（丢掉第 2 页），第 1 页转 90、第 4 页转 270
    let ops = vec![
        PageOp { page: 3, rotate: 0 },
        PageOp { page: 1, rotate: 90 },
        PageOp { page: 4, rotate: 270 },
    ];
    match arrange_pages(&src, &ops) {
        Ok((dst, n)) => {
            check("导出 3 页", n == 3, format!("{n} 页"));
            let pages = read_pages(&dst);
            check(
                "顺序按清单重排",
                pages.iter().map(|p| p.0).collect::<Vec<_>>() == vec![130, 110, 140],
                format!("宽度序 {:?}", pages.iter().map(|p| p.0).collect::<Vec<_>>()),
            );
            check(
                "丢掉的第 2 页没进去",
                !pages.iter().any(|p| p.0 == 120),
                "宽度 120 那页不在".into(),
            );
            check(
                "逐页旋转各自生效",
                pages.iter().map(|p| p.1).collect::<Vec<_>>() == vec![0, 90, 270],
                format!("旋转序 {:?}", pages.iter().map(|p| p.1).collect::<Vec<_>>()),
            );
        }
        Err(e) => check("导出 3 页", false, format!("报错 {}", e.key)),
    }

    // 一页都不留 → 明确报错，而不是产出一份空 PDF
    check(
        "一页都没选时报错",
        matches!(arrange_pages(&src, &[]), Err(e) if e.key == "err.pdfNoPagesPicked"),
        "err.pdfNoPagesPicked".into(),
    );

    // 单页提取 + 旋转累加：2 页都初始转 90，只留第 2 页再 +90 → 应得 180，且输出 1 页可读
    let pre = tmp.join("已转.pdf");
    make_pdf_rot(&pre, &[200, 210], &[90, 90]);
    match arrange_pages(&pre, &[PageOp { page: 2, rotate: 90 }]) {
        Ok((dst, n)) => {
            let pages = read_pages(&dst);
            check(
                "只留一页也能正常输出",
                n == 1 && pages.len() == 1,
                format!("导出 {n} 页、读回 {} 页", pages.len()),
            );
            let r = pages.first().map(|p| p.1).unwrap_or(-1);
            check("旋转在原有角度上累加", r == 180, format!("90 + 90 = {r}"));
        }
        Err(e) => check("旋转在原有角度上累加", false, format!("报错 {}", e.key)),
    }

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}
