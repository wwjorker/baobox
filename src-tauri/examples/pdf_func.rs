//! PDF 工具功能验收：产出的文件必须真的能被重新打开、页数正确。
//!
//! 只看「没报错」是不够的——PDF 很容易写出一份结构损坏、
//! 写入时不报错但阅读器打不开的文件。所以每一步都把产物再读回来验。

use baobox_lib::pdf_ops::{merge_files, rotate_file, split_file, text_file};
use lopdf::Document;
use std::path::{Path, PathBuf};

fn pages_of(p: &Path) -> Result<usize, String> {
    Document::load(p)
        .map(|d| d.get_pages().len())
        .map_err(|e| e.to_string())
}

fn main() {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| r"C:\baobox-samples\pdftest".into()),
    );

    let mut srcs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("测试目录不存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "pdf").unwrap_or(false))
        .collect();
    srcs.sort();

    println!("======== PDF 工具功能验收 ========\n输入:");
    let mut expect_total = 0usize;
    for s in &srcs {
        let n = pages_of(s).unwrap_or(0);
        expect_total += n;
        println!("  {:<12} {n} 页", s.file_name().unwrap().to_string_lossy());
    }
    println!("  合计 {expect_total} 页\n验收:");

    let mut pass = 0;
    let mut fail = 0;
    let mut check = |label: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {label:<10} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {label:<10} {detail}");
        }
    };

    // ---- 合并：产物页数必须等于各输入之和 ----
    match merge_files(&srcs, &srcs[0]) {
        Ok((dst, claimed)) => match pages_of(&dst) {
            Ok(actual) => check(
                "合并",
                actual == expect_total && claimed == expect_total,
                format!("产物实测 {actual} 页，期望 {expect_total} 页"),
            ),
            Err(e) => check("合并", false, format!("产物读不回来: {e}")),
        },
        Err(e) => check("合并", false, format!("{:?}", e.key)),
    }

    // ---- 拆分：每份产物都必须是 1 页 ----
    match split_file(&srcs[0]) {
        Ok((_, total)) => {
            let out = dir.join("Baobox_output");
            let singles: Vec<PathBuf> = std::fs::read_dir(&out)
                .map(|rd| {
                    rd.filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.to_string_lossy().contains("第"))
                        .collect()
                })
                .unwrap_or_default();
            let all_one = !singles.is_empty() && singles.iter().all(|p| pages_of(p) == Ok(1));
            check(
                "拆分",
                all_one && singles.len() >= total,
                format!("{} 份产物，每份 1 页: {all_one}", singles.len()),
            );
        }
        Err(e) => check("拆分", false, format!("{:?}", e.key)),
    }

    // ---- 旋转：/Rotate 必须真的写进每一页 ----
    match rotate_file(&srcs[0], 90) {
        Ok((dst, _)) => match Document::load(&dst) {
            Ok(doc) => {
                let angles: Vec<i64> = doc
                    .get_pages()
                    .values()
                    .filter_map(|id| doc.get_object(*id).ok())
                    .filter_map(|o| o.as_dict().ok())
                    .map(|d| d.get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0))
                    .collect();
                let ok = !angles.is_empty() && angles.iter().all(|a| *a == 90);
                check("旋转", ok, format!("各页 /Rotate = {angles:?}"));
            }
            Err(e) => check("旋转", false, format!("产物读不回来: {e}")),
        },
        Err(e) => check("旋转", false, format!("{:?}", e.key)),
    }

    // ---- 提取文字：必须捞到我们写进去的标记文本 ----
    match text_file(&srcs[0]) {
        Ok((_, text)) => check(
            "提取文字",
            text.contains("Baobox"),
            format!(
                "{} 个字符，含预期标记: {}",
                text.chars().count(),
                text.contains("Baobox")
            ),
        ),
        Err(e) => check("提取文字", false, format!("{:?}", e.key)),
    }

    println!("\n======== 通过 {pass} / 失败 {fail} ========");
}
