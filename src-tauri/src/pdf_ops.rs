use crate::batch::{run_batch, FileOutcome};
use crate::err::{AppError, AppResult};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 打开一份 PDF，把 lopdf 的错误翻译成用户看得懂的提示。
///
/// 真实世界的 PDF 极其混乱，报错必须指向「下一步该怎么办」，
/// 而不是把解析器的内部消息原样抛出去（方案风险 4 与 19）。
fn open(src: &Path) -> AppResult<Document> {
    let doc = Document::load(long_path(src)).map_err(|e| {
        let msg = e.to_string().to_lowercase();
        if msg.contains("encrypt") {
            AppError::new("err.encrypted")
        } else {
            AppError::decode("PDF", e)
        }
    })?;
    if doc.is_encrypted() {
        return Err(AppError::new("err.encrypted"));
    }
    Ok(doc)
}

fn save(doc: &mut Document, dst: &Path) -> AppResult<()> {
    // 源文件若带增量更新，trailer 里会留下 /Prev 和 /XRefStm 指向原文件中的
    // 字节偏移。我们整份重写后这些偏移全部失效，产物写入时不报错，
    // 但阅读器一打开就是「交叉引用表无效」。必须先清掉。
    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"XRefStm");
    doc.renumber_objects();
    doc.compress();
    doc.save(long_path(dst)).map_err(|e| AppError::unknown(e))?;
    Ok(())
}

// ================================================================ 合并

/// 把一批路径合并成一份 PDF，返回产物路径和总页数。
pub fn merge_files(srcs: &[PathBuf], out_dir_hint: &Path) -> AppResult<(PathBuf, usize)> {
    let docs: Vec<Document> = srcs
        .iter()
        .map(|p| open(p))
        .collect::<AppResult<Vec<_>>>()?;
    let pages: usize = docs.iter().map(|d| d.get_pages().len()).sum();
    let mut merged = merge_docs(docs)?;
    let dir = output_dir_for(out_dir_hint)?;
    let dst = unique_path(
        &dir,
        &format!("{} 等 {} 份合并", stem_of(out_dir_hint), srcs.len()),
        "pdf",
    );
    save(&mut merged, &dst)?;
    Ok((dst, pages))
}

/// 把多份 PDF 合并成一份。
///
/// lopdf 没有现成的 merge，需要手工重编号对象再重建页树——
/// 直接拼接会因为对象 ID 冲突而产出一份打不开的文件。
pub fn merge_docs(docs: Vec<Document>) -> AppResult<Document> {
    let mut max_id = 1u32;
    let mut pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();

    for mut doc in docs {
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        pages.extend(
            doc.get_pages()
                .into_iter()
                .filter_map(|(_, id)| doc.get_object(id).ok().map(|o| (id, o.to_owned()))),
        );
        objects.extend(doc.objects.clone());
    }

    let mut out = Document::with_version("1.5");
    out.objects = objects;

    // 页树的根节点要重建，各来源文档自带的 Pages / Catalog 一律丢弃
    let pages_id = out.new_object_id();
    let kids: Vec<Object> = pages.keys().map(|id| Object::Reference(*id)).collect();
    if kids.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }

    for (id, obj) in &pages {
        if let Ok(dict) = obj.as_dict() {
            let mut d = dict.clone();
            d.set("Parent", pages_id);
            out.objects.insert(*id, Object::Dictionary(d));
        }
    }

    let mut pages_dict = lopdf::Dictionary::new();
    pages_dict.set("Type", "Pages");
    pages_dict.set("Count", kids.len() as u32);
    pages_dict.set("Kids", kids);
    out.objects.insert(pages_id, Object::Dictionary(pages_dict));

    let mut catalog = lopdf::Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", pages_id);
    let catalog_id = out.add_object(Object::Dictionary(catalog));

    out.trailer.set("Root", catalog_id);
    out.max_id = out.objects.len() as u32;
    out.renumber_objects();
    out.adjust_zero_pages();
    Ok(out)
}

fn pdf_merge_blocking(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    if paths.is_empty() {
        return Vec::new();
    }

    let mut docs = Vec::new();
    let mut outcomes = Vec::new();
    let total = paths.len();

    for (i, p) in paths.iter().enumerate() {
        let src = PathBuf::from(p);
        match open(&src) {
            Ok(d) => docs.push((src, d)),
            Err(e) => {
                let o = FileOutcome::fail(&src, e);
                crate::batch::emit(&app, i, total, &o);
                outcomes.push(o);
            }
        }
    }

    if docs.is_empty() {
        return outcomes;
    }

    let first = docs[0].0.clone();
    let count: usize = docs.iter().map(|(_, d)| d.get_pages().len()).sum();
    let result = (|| -> AppResult<PathBuf> {
        let mut merged = merge_docs(docs.iter().map(|(_, d)| d.clone()).collect())?;
        let dir = output_dir_for(&first)?;
        let dst = unique_path(&dir, &format!("{} 等 {} 份合并", stem_of(&first), total), "pdf");
        save(&mut merged, &dst)?;
        Ok(dst)
    })();

    // 合并的产物是一份文件，把它挂在第一个输入上汇报
    let o = match result {
        Ok(dst) => FileOutcome::ok(&first, dst, Some(format!("{} 份 · 共 {count} 页", docs.len()))),
        Err(e) => FileOutcome::fail(&first, e),
    };
    crate::batch::emit(&app, total - 1, total, &o);
    outcomes.insert(0, o);
    outcomes
}

// ================================================================ 拆分

/// 把一份 PDF 拆成每页一份。返回最后一份的路径和总页数。
pub fn split_file(src: &Path) -> AppResult<(PathBuf, usize)> {
    let doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let dir = output_dir_for(src)?;
    let stem = stem_of(src);
    let total = pages.len();
    let mut last = dir.clone();

    for page_no in pages.keys() {
        let mut single = doc.clone();
        let drop: Vec<u32> = pages.keys().copied().filter(|n| n != page_no).collect();
        single.delete_pages(&drop);
        single.adjust_zero_pages();
        let dst = unique_path(&dir, &format!("{stem} 第{page_no}页"), "pdf");
        save(&mut single, &dst)?;
        last = dst;
    }
    Ok((last, total))
}

fn pdf_split_blocking(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    run_batch(&app, paths, |src| {
        let (last, total) = split_file(src)?;
        Ok((last, Some(format!("{total} 页 → {total} 份"))))
    })
}

// ================================================================ 旋转

pub fn rotate_file(src: &Path, degrees: i64) -> AppResult<(PathBuf, usize)> {
    let mut doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let n = pages.len();
    for id in pages.values() {
        if let Ok(obj) = doc.get_object_mut(*id) {
            if let Ok(dict) = obj.as_dict_mut() {
                // /Rotate 是累加的：已有 90 再转 90 应得 180
                let cur = dict.get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0);
                let next = ((cur + degrees) % 360 + 360) % 360;
                dict.set("Rotate", next);
            }
        }
    }
    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 旋转{}度", stem_of(src), degrees), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, n))
}

fn pdf_rotate_blocking(app: AppHandle, paths: Vec<String>, degrees: i64) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (dst, n) = rotate_file(src, degrees)?;
        Ok((dst, Some(format!("{n} 页 · 旋转 {degrees}°"))))
    })
}

// ========================================================== 图片转 PDF

fn pdf_from_image_blocking(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    if paths.is_empty() {
        return Vec::new();
    }
    let first = PathBuf::from(&paths[0]);
    let total = paths.len();

    let result = (|| -> AppResult<(PathBuf, usize)> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();

        for p in &paths {
            let src = PathBuf::from(p);
            let img = lopdf::xobject::image(long_path(&src))
                .map_err(|e| AppError::decode("图片", e))?;
            // 用图片自身的像素尺寸当页面尺寸，避免留白或裁切
            let w = img.dict.get(b"Width").and_then(|o| o.as_i64()).unwrap_or(595);
            let h = img.dict.get(b"Height").and_then(|o| o.as_i64()).unwrap_or(842);

            let mut page = lopdf::Dictionary::new();
            page.set("Type", "Page");
            page.set("Parent", pages_id);
            page.set(
                "MediaBox",
                vec![0.into(), 0.into(), Object::Integer(w), Object::Integer(h)],
            );
            let page_id = doc.add_object(Object::Dictionary(page));
            doc.add_page_contents(page_id, Vec::new())
                .map_err(|e| AppError::unknown(e))?;
            doc.insert_image(page_id, img, (0.0, 0.0), (w as f32, h as f32))
                .map_err(|e| AppError::unknown(e))?;
            kids.push(Object::Reference(page_id));
        }

        let mut pages_dict = lopdf::Dictionary::new();
        pages_dict.set("Type", "Pages");
        pages_dict.set("Count", kids.len() as u32);
        pages_dict.set("Kids", kids);
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

        let mut catalog = lopdf::Dictionary::new();
        catalog.set("Type", "Catalog");
        catalog.set("Pages", pages_id);
        let catalog_id = doc.add_object(Object::Dictionary(catalog));
        doc.trailer.set("Root", catalog_id);

        let dir = output_dir_for(&first)?;
        let dst = unique_path(&dir, &format!("{} 等 {} 张", stem_of(&first), total), "pdf");
        save(&mut doc, &dst)?;
        Ok((dst, total))
    })();

    let o = match result {
        Ok((dst, n)) => FileOutcome::ok(&first, dst, Some(format!("{n} 张 → {n} 页"))),
        Err(e) => FileOutcome::fail(&first, e),
    };
    crate::batch::emit(&app, 0, 1, &o);
    vec![o]
}

// ========================================================== PDF 转文本

pub fn text_file(src: &Path) -> AppResult<(PathBuf, String)> {
    let doc = open(src)?;
    let nums: Vec<u32> = doc.get_pages().keys().copied().collect();
    let text = doc
        .extract_text(&nums)
        .map_err(|e| AppError::decode("PDF", e))?;
    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &stem_of(src), "txt");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(text.as_bytes());
    std::fs::write(long_path(&dst), &bytes)?;
    Ok((dst, text))
}

fn pdf_to_text_blocking(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    let total = paths.len();
    let mut out = Vec::with_capacity(total);
    for (index, p) in paths.iter().enumerate() {
        let src = PathBuf::from(p);
        let outcome = match text_file(&src) {
            Ok((dst, text)) => {
                let chars = text.chars().count();
                let mut o = FileOutcome::ok(&src, dst, Some(format!("提取 {chars} 个字符")));
                o.text = Some(text);
                o
            }
            Err(e) => FileOutcome::fail(&src, e),
        };
        crate::batch::emit(&app, index, total, &outcome);
        out.push(outcome);
    }
    out
}

// ==================================================== 异步命令入口

#[tauri::command]
pub async fn pdf_merge(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_merge_blocking(app, paths))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_split(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_split_blocking(app, paths))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_rotate(app: AppHandle, paths: Vec<String>, degrees: i64) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_rotate_blocking(app, paths, degrees))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_from_image(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_from_image_blocking(app, paths))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_to_text(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_to_text_blocking(app, paths))
        .await
        .unwrap_or_default()
}
