//! 摸清真实 PDF 里内嵌图片的编码分布，决定压缩该支持哪几种。
//!
//! 不同 Filter 的处理难度差很多：DCTDecode 本身就是 JPEG，可以直接
//! 解码重编码；FlateDecode 是裸像素，得靠 Width/Height/ColorSpace/
//! BitsPerComponent 自己拼回图像。先看看哪种占大头再决定投入。

use lopdf::{Document, Object};
use std::collections::BTreeMap;

fn main() {
    let list = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"C:\baobox-samples\pdf_list_real.txt".into());
    let paths: Vec<String> = std::fs::read_to_string(&list)
        .expect("读不到清单")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .take(300)
        .collect();

    let mut filters: BTreeMap<String, usize> = BTreeMap::new();
    let mut bytes_by_filter: BTreeMap<String, u64> = BTreeMap::new();
    let mut docs_with_images = 0usize;
    let mut docs_scanned = 0usize;
    let mut total_images = 0usize;

    for p in &paths {
        let Ok(doc) = Document::load(p) else { continue };
        docs_scanned += 1;
        let mut here = 0usize;

        for (_, obj) in doc.objects.iter() {
            let Object::Stream(s) = obj else { continue };
            let is_image = s
                .dict
                .get(b"Subtype")
                .and_then(|o| o.as_name())
                .map(|n| n == b"Image")
                .unwrap_or(false);
            if !is_image {
                continue;
            }
            here += 1;
            total_images += 1;

            let f = match s.dict.get(b"Filter") {
                Ok(Object::Name(n)) => String::from_utf8_lossy(n).to_string(),
                Ok(Object::Array(a)) => a
                    .iter()
                    .filter_map(|o| o.as_name().ok())
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .collect::<Vec<_>>()
                    .join("+"),
                _ => "无".into(),
            };
            *filters.entry(f.clone()).or_default() += 1;
            *bytes_by_filter.entry(f).or_default() += s.content.len() as u64;
        }
        if here > 0 {
            docs_with_images += 1;
        }
    }

    println!("======== PDF 内嵌图片编码分布 ========");
    println!("扫描 {docs_scanned} 份，其中 {docs_with_images} 份含图片，共 {total_images} 张\n");
    let mut v: Vec<_> = filters.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1));
    println!("{:<28} {:>7}  {:>10}", "Filter", "张数", "总体积");
    for (f, n) in v {
        let mb = *bytes_by_filter.get(f).unwrap_or(&0) as f64 / 1_048_576.0;
        println!("{f:<28} {n:>7}  {mb:>9.1} MB");
    }
}
