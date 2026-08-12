//! 扫描件转可搜索 PDF。
//!
//! 这是整个产品最核心的差异化点上缺的最后一块。iLovePDF 和 Smallpdf 把
//! OCR 锁在付费订阅里，而 Windows 自带的识别引擎调它零成本——但只吐出
//! 一段纯文本，用户拿到的还是那份搜不了的扫描件。真正有用的是**原样保留
//! 页面外观，同时能搜能选能复制**。
//!
//! # 做法
//!
//! 每一页：渲染成图 → 识别出每个词及其像素坐标 → 把这些词按原位、
//! 用「不可见」渲染模式（Tr 3）盖在原内容之上。页面看上去分毫未变，
//! 但阅读器的搜索和选取能落在对的位置上。
//!
//! # 两个容易做错的地方
//!
//! **坐标系是反的。** 图片原点在左上，PDF 用户空间原点在左下。
//! 弄反了文字会整页上下颠倒地贴，而且不会报任何错——因为文字是隐形的，
//! 只有真去搜索或拖选才会发现全错位了。
//!
//! **字宽对不上。** 识别出的词在图上占多宽是已知的，而同一串字用嵌入
//! 字体排出来是另一个宽度。不做校正的话，拖选的高亮框会跟看到的字错开
//! 越来越多。用水平缩放（Tz）把每一段拉到该有的宽度。

use crate::batch::{FileOutcome, Note};
use crate::err::{AppError, AppResult};
use crate::ocr::{recognize_words, OcrWord};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use lopdf::{Document, Object};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 渲染分辨率。太低识别不准，太高又慢又占内存；
/// 300 DPI 是扫描件的常规分辨率，A4 宽度约 2480 像素。
const RENDER_WIDTH: u32 = 2000;

/// 给一份 PDF 加上不可见文字层。返回产物路径、页数、识别出的词数。
pub fn make_searchable(
    src: &Path,
    lang: Option<&str>,
    app: Option<&AppHandle>,
) -> AppResult<(PathBuf, usize, usize)> {
    let mut doc = Document::load(long_path(src)).map_err(|e| {
        if e.to_string().to_lowercase().contains("encrypt") {
            AppError::new("err.encrypted")
        } else {
            AppError::decode("PDF", e)
        }
    })?;
    if doc.is_encrypted() {
        return Err(AppError::new("err.encrypted"));
    }

    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let page_count = pages.len();

    // 先把所有页认一遍，收齐用到的字符再一次性嵌字体——
    // 逐页嵌会在文件里堆出几十份重复的字体子集。
    let tmp = std::env::temp_dir().join(format!("baobox_ocr_{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;

    let mut per_page: Vec<(lopdf::ObjectId, Vec<OcrWord>, f32, f32, f32)> = Vec::new();
    let mut all_text = String::new();

    for (i, (_, page_id)) in pages.iter().enumerate() {
        if let Some(a) = app {
            crate::batch::emit_working(a, i, page_count, &format!("{}/{}", i + 1, page_count));
        }
        if crate::batch::cancelled() {
            break;
        }

        let png = match crate::pdf_render::render_page(src, i as u32, RENDER_WIDTH) {
            Ok(b) => b,
            // 某一页渲染不出来不该让整份文件失败，跳过它，其余照做
            Err(_) => continue,
        };
        let img_path = tmp.join(format!("p{i}.png"));
        std::fs::write(&img_path, &png)?;

        let page = match recognize_words(&img_path, lang) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let _ = std::fs::remove_file(&img_path);

        if page.words.is_empty() || page.img_w == 0 {
            continue;
        }

        let (px, py, pw, ph) = page_box(&doc, *page_id);
        // 图片像素 → PDF 点
        let scale = pw / page.img_w as f32;

        for w in &page.words {
            all_text.push_str(&w.text);
        }
        per_page.push((*page_id, page.words, scale, px, py + ph));
    }

    let _ = std::fs::remove_dir_all(&tmp);

    let word_count: usize = per_page.iter().map(|(_, w, _, _, _)| w.len()).sum();
    if word_count == 0 {
        return Err(AppError::new("err.ocrNothingFound"));
    }

    let font = crate::pdf_font::prepare(&all_text)?;
    let font_id = crate::pdf_font::embed(&mut doc, &font);

    for (page_id, words, scale, origin_x, top_y) in per_page {
        let content = build_layer(&words, scale, origin_x, top_y, &font);
        let stream = lopdf::Stream::new(lopdf::Dictionary::new(), content.into_bytes());
        let layer_id = doc.add_object(Object::Stream(stream));
        append_content(&mut doc, page_id, layer_id);
        crate::pdf_font::attach_resources(&mut doc, page_id, font_id, None);
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 可搜索", stem_of(src)), "pdf");
    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"XRefStm");
    doc.renumber_objects();
    doc.compress();
    doc.save(long_path(&dst))
        .map_err(|e| AppError::unknown(e))?;
    Ok((dst, page_count, word_count))
}

/// 生成这一页的不可见文字层。
fn build_layer(
    words: &[OcrWord],
    scale: f32,
    origin_x: f32,
    top_y: f32,
    font: &crate::pdf_font::EmbeddedFont,
) -> String {
    let mut s = String::from("q\n");
    for w in words {
        if w.text.trim().is_empty() || w.h <= 0.0 {
            continue;
        }
        // 字号取词的高度。真实字形通常比外接框略小，0.8 是常用的经验值，
        // 目的是让选取高亮的高度看起来跟图上的字差不多。
        let size = (w.h * scale * 0.8).max(1.0);
        let x = origin_x + w.x * scale;
        // 图的原点在左上、PDF 在左下，这里要翻过来。
        // 基线大致在词框底部往上一点。
        let y = top_y - (w.y + w.h) * scale + size * 0.18;

        let natural = font.width_of_text(&w.text, size);
        let want = w.w * scale;
        // 把这段横向拉到跟图上一样宽，否则拖选的框会越错越远
        let tz = if natural > 0.01 {
            (want / natural * 100.0).clamp(10.0, 1000.0)
        } else {
            100.0
        };

        // Tr 3 = 既不填充也不描边，即完全不可见。
        // 字体名要跟 attach_resources 里注册的一致，否则这一层引用不到字体。
        s.push_str(&format!(
            "BT 3 Tr /BaoboxF {size:.2} Tf {tz:.1} Tz {x:.2} {y:.2} Td <{}> Tj ET\n",
            font.encode(&w.text)
        ));
    }
    s.push_str("Q\n");
    s
}

/// 把新内容追加到页面已有的内容之后，不替换。
fn append_content(doc: &mut Document, page_id: lopdf::ObjectId, layer_id: lopdf::ObjectId) {
    let Ok(page) = doc.get_object(page_id).and_then(|o| o.as_dict().cloned()) else {
        return;
    };
    let mut list: Vec<Object> = match page.get(b"Contents") {
        Ok(Object::Reference(r)) => vec![Object::Reference(*r)],
        Ok(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    list.push(Object::Reference(layer_id));
    if let Ok(Object::Dictionary(d)) = doc.get_object_mut(page_id) {
        d.set("Contents", Object::Array(list));
    }
}

/// 取页面的绘制区域。MediaBox 可能带非零原点，
/// 当成 0 会让整层文字偏移一个页边距。
fn page_box(doc: &Document, page_id: lopdf::ObjectId) -> (f32, f32, f32, f32) {
    let read = |key: &[u8]| -> Option<Vec<f32>> {
        let d = doc.get_object(page_id).ok()?.as_dict().ok()?;
        let arr = match d.get(key).ok()? {
            Object::Array(a) => a.clone(),
            Object::Reference(r) => doc.get_object(*r).ok()?.as_array().ok()?.clone(),
            _ => return None,
        };
        let v: Vec<f32> = arr
            .iter()
            .filter_map(|o| match o {
                Object::Integer(i) => Some(*i as f32),
                Object::Real(f) => Some(*f),
                _ => None,
            })
            .collect();
        (v.len() == 4).then_some(v)
    };
    let b = read(b"CropBox")
        .or_else(|| read(b"MediaBox"))
        .unwrap_or_else(|| {
            // A4，跟渲染引擎在缺 MediaBox 时的默认一致
            vec![0.0, 0.0, 595.0, 842.0]
        });
    let (x0, y0, x1, y1) = (
        b[0].min(b[2]),
        b[1].min(b[3]),
        b[0].max(b[2]),
        b[1].max(b[3]),
    );
    (x0, y0, x1 - x0, y1 - y0)
}

fn blocking(app: AppHandle, paths: Vec<String>, lang: Option<String>) -> Vec<FileOutcome> {
    let total = paths.len();
    let mut out = Vec::with_capacity(total);
    crate::batch::reset_cancel();

    for (index, p) in paths.iter().enumerate() {
        let src = PathBuf::from(p);
        if crate::batch::cancelled() {
            let o = FileOutcome::skipped(&src, "run.cancelledSkip");
            crate::batch::emit(&app, index, total, &o);
            out.push(o);
            continue;
        }

        let o = match make_searchable(&src, lang.as_deref(), Some(&app)) {
            Ok((dst, pages, words)) => FileOutcome::ok(
                &src,
                dst,
                Some(
                    Note::new("note.ocrLayer")
                        .with("pages", pages)
                        .with("words", words),
                ),
            ),
            Err(e) => FileOutcome::fail(&src, e),
        };
        crate::batch::emit(&app, index, total, &o);
        out.push(o);
    }
    out
}

#[tauri::command]
pub async fn pdf_ocr_layer(
    app: AppHandle,
    paths: Vec<String>,
    lang: Option<String>,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || blocking(app, paths, lang))
        .await
        .unwrap_or_default()
}
