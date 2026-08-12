use crate::batch::{run_batch, FileOutcome, Note};
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

/// 移除 PDF 的打开密码。
///
/// 要求用户提供正确密码——这是「我知道自己文件的密码，但它挡着我编辑」
/// 这个常见需求，不是破解。密码错了就明确报错，不做任何猜测或穷举。
pub fn decrypt_file(src: &Path, password: &str) -> AppResult<(PathBuf, bool)> {
    let mut doc = Document::load(long_path(src)).map_err(|e| AppError::decode("PDF", e))?;
    let was_encrypted = doc.is_encrypted();

    if was_encrypted {
        doc.decrypt(password)
            .map_err(|e| AppError::new("err.pdfWrongPassword").detail(e))?;
        // 解密后必须把加密字典从 trailer 里摘掉，否则产物仍标称自己是加密的，
        // 阅读器会拿已经失效的密钥去解已经明文的内容，直接打不开。
        doc.trailer.remove(b"Encrypt");
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 已解密", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, was_encrypted))
}

fn pdf_decrypt_blocking(app: AppHandle, paths: Vec<String>, password: String) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (dst, was) = decrypt_file(src, &password)?;
        Ok((
            dst,
            Some(Note::new(if was {
                "note.decrypted"
            } else {
                "note.decryptNotNeeded"
            })),
        ))
    })
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
    // `new_object_id` only increments `max_id`; assigning an existing object
    // map does not update it.  Keep it in sync before allocating the new page
    // tree, otherwise Pages/Catalog can overwrite imported objects.
    out.max_id = out.objects.keys().map(|id| id.0).max().unwrap_or(0);

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
        let dst = unique_path(
            &dir,
            &format!("{} 等 {} 份合并", stem_of(&first), total),
            "pdf",
        );
        save(&mut merged, &dst)?;
        Ok(dst)
    })();

    // 产物只有一份，挂在第一个输入上；其余每一个也要各自发一条「已并入」。
    // 早先只发第一条，界面上后面几行就永远停在「等待」，看着像处理到一半卡死了。
    let o = match result {
        Ok(dst) => FileOutcome::ok(
            &first,
            dst,
            Some(
                Note::new("note.merged")
                    .with("files", docs.len())
                    .with("pages", count),
            ),
        ),
        Err(e) => FileOutcome::fail(&first, e),
    };
    let rest: Vec<PathBuf> = docs.iter().skip(1).map(|(p, _)| p.clone()).collect();
    for (i, o) in crate::batch::fold_outcomes(o, &rest)
        .into_iter()
        .enumerate()
    {
        crate::batch::emit(&app, i, total, &o);
        outcomes.insert(i, o);
    }
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
        Ok((last, Some(Note::new("note.pdfSplit").with("total", total))))
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
        Ok((
            dst,
            Some(
                Note::new("note.pdfRotate")
                    .with("n", n)
                    .with("deg", degrees),
            ),
        ))
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
            let img =
                lopdf::xobject::image(long_path(&src)).map_err(|e| AppError::decode("图片", e))?;
            // 用图片自身的像素尺寸当页面尺寸，避免留白或裁切
            let w = img
                .dict
                .get(b"Width")
                .and_then(|o| o.as_i64())
                .unwrap_or(595);
            let h = img
                .dict
                .get(b"Height")
                .and_then(|o| o.as_i64())
                .unwrap_or(842);

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

    // 同合并：产物一份挂第一个，其余每张各发一条，别让它们停在「等待」
    let o = match result {
        Ok((dst, n)) => FileOutcome::ok(&first, dst, Some(Note::new("note.imgToPdf").with("n", n))),
        Err(e) => FileOutcome::fail(&first, e),
    };
    let rest: Vec<PathBuf> = paths.iter().skip(1).map(PathBuf::from).collect();
    let outcomes = crate::batch::fold_outcomes(o, &rest);
    for (i, o) in outcomes.iter().enumerate() {
        crate::batch::emit(&app, i, total, o);
    }
    outcomes
}

// ============================================================ 压缩

/// 重新编码一张内嵌图片。返回 (原字节数, 新字节数)，不该动或压不小就返回 None。
///
/// 实测 300 份真实 PDF 的 6854 张内嵌图片：DCTDecode 3453 张 297 MB，
/// FlateDecode 3358 张 407 MB。裸像素那类按体积反而更大，
/// 所以两种都得处理，只做 JPEG 会漏掉一半以上的可压缩字节。
fn recompress_image(stream: &mut lopdf::Stream, quality: u8) -> Option<(usize, usize)> {
    // 有软掩码说明带透明通道，转成 JPEG 会把它丢掉
    if stream.dict.get(b"SMask").is_ok() || stream.dict.get(b"Mask").is_ok() {
        return None;
    }
    let w = stream.dict.get(b"Width").ok()?.as_i64().ok()? as u32;
    let h = stream.dict.get(b"Height").ok()?.as_i64().ok()? as u32;
    if w == 0 || h == 0 || w > 20000 || h > 20000 {
        return None;
    }
    // 1 位的黑白位图转 JPEG 只会更大更糊，扫描件里这类不少
    let bpc = stream
        .dict
        .get(b"BitsPerComponent")
        .and_then(|o| o.as_i64())
        .unwrap_or(8);
    if bpc != 8 {
        return None;
    }

    let filter = crate::pdf_img::single_filter(stream)?;

    let before = stream.content.len();
    let img: image::DynamicImage = match filter.as_str() {
        "DCTDecode" => {
            image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg).ok()?
        }
        // 裸像素这条路要自己解压：lopdf 对 Subtype=Image 一律拒绝解压，
        // 而这半边按体积比 JPEG 那半边还大。详见 pdf_img.rs。
        "FlateDecode" => {
            let raw = crate::pdf_img::raw_pixels(stream)?;
            crate::pdf_img::to_image(stream, &raw, w, h)?
        }
        _ => return None,
    };

    let jpeg = crate::image_ops::encode_jpeg(&img, quality).ok()?;
    // 压不小就保持原样——用户点「压缩」结果变大是说不过去的
    if jpeg.len() >= before {
        return None;
    }

    let gray = matches!(img, image::DynamicImage::ImageLuma8(_));
    stream.set_plain_content(jpeg.clone());
    stream
        .dict
        .set("Filter", Object::Name(b"DCTDecode".to_vec()));
    stream.dict.set(
        "ColorSpace",
        Object::Name(if gray {
            b"DeviceGray".to_vec()
        } else {
            b"DeviceRGB".to_vec()
        }),
    );
    stream.dict.set("BitsPerComponent", 8i64);
    stream.dict.remove(b"DecodeParms");
    Some((before, jpeg.len()))
}

pub fn compress_file(src: &Path, quality: u8) -> AppResult<(PathBuf, usize, u64)> {
    let mut doc = open(src)?;
    let ids: Vec<lopdf::ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| {
            matches!(o, Object::Stream(s) if s
                .dict
                .get(b"Subtype")
                .and_then(|x| x.as_name())
                .map(|n| n == b"Image")
                .unwrap_or(false))
        })
        .map(|(id, _)| *id)
        .collect();

    let mut touched = 0usize;
    let mut saved: u64 = 0;
    for id in ids {
        if let Some(Object::Stream(s)) = doc.objects.get_mut(&id) {
            if let Some((before, after)) = recompress_image(s, quality) {
                touched += 1;
                saved += (before - after) as u64;
            }
        }
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 压缩", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, touched, saved))
}

fn pdf_compress_blocking(app: AppHandle, paths: Vec<String>, quality: u8) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (dst, touched, _) = compress_file(src, quality)?;
        Ok((
            dst,
            Some(if touched == 0 {
                Note::new("note.pdfStructOnly")
            } else {
                Note::new("note.pdfRecompressed").with("n", touched)
            }),
        ))
    })
}

// ================================================ 提取内嵌图片

/// 把 PDF 里的图片原样抠出来。
///
/// 关键在「原样」：DCTDecode 的流本身就是一份完整 JPEG，直接写盘即可，
/// 不重新编码——渲染整页再截图会掉一轮画质，而这个功能的用途
/// （拿回设计稿里的原图、抽出扫描件的每一页）恰恰要的是原始像素。
pub fn extract_images(src: &Path, min_px: u32) -> AppResult<(PathBuf, usize)> {
    let doc = open(src)?;
    let dir = output_dir_for(src)?;
    let stem = stem_of(src);

    let mut n = 0usize;
    let mut last = dir.clone();
    // 按对象号排序，产物编号才是稳定的；HashMap 的遍历顺序每次都不一样
    let mut ids: Vec<lopdf::ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| {
            matches!(o, Object::Stream(s) if s
                .dict
                .get(b"Subtype")
                .and_then(|x| x.as_name())
                .map(|k| k == b"Image")
                .unwrap_or(false))
        })
        .map(|(id, _)| *id)
        .collect();
    ids.sort();

    for id in ids {
        let Some(Object::Stream(s)) = doc.objects.get(&id) else {
            continue;
        };
        let w = s.dict.get(b"Width").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        let h = s.dict.get(b"Height").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        // 图标、分隔线、扫描噪点会有成百上千个，全导出来只是垃圾
        if w < min_px || h < min_px {
            continue;
        }

        let Some(filter) = crate::pdf_img::single_filter(s) else {
            continue;
        };

        let (bytes, ext): (Vec<u8>, &str) = match filter.as_str() {
            // 已经是 JPEG，原样落盘，一个字节都不动 —— 这个功能的用途
            // （拿回设计稿里的原图、抽出扫描件每一页）要的就是原始像素
            "DCTDecode" => (s.content.clone(), "jpg"),
            // JPX 是 JPEG 2000，多数看图软件打不开，但比不导出好
            "JPXDecode" => (s.content.clone(), "jp2"),
            "FlateDecode" => {
                let Some(raw) = crate::pdf_img::raw_pixels(s) else {
                    continue;
                };
                let Some(img) = crate::pdf_img::to_image(s, &raw, w, h) else {
                    continue;
                };
                match crate::image_ops::encode(&img, crate::image_ops::OutFmt::Png, 100) {
                    Ok(b) => (b, "png"),
                    Err(_) => continue,
                }
            }
            _ => continue,
        };

        n += 1;
        let dst = unique_path(&dir, &format!("{stem} 图{n:03}"), ext);
        std::fs::write(long_path(&dst), &bytes)?;
        last = dst;
    }

    if n == 0 {
        return Err(AppError::new("err.pdfNoImages"));
    }
    Ok((last, n))
}

fn pdf_extract_images_blocking(
    app: AppHandle,
    paths: Vec<String>,
    min_px: u32,
) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (last, n) = extract_images(src, min_px)?;
        Ok((last, Some(Note::new("note.pdfExtracted").with("n", n))))
    })
}

// ================================================ 页面重排与删除

/// 解析「1,3,5-8」这类页码范围，返回 1 基页号。
///
/// 用户会写各种花样：空格、中文逗号、倒着写的区间。都认，
/// 因为在这上面报错只会让人烦躁，而意图是明确的。
pub fn parse_pages(spec: &str, total: u32) -> AppResult<Vec<u32>> {
    let mut out = Vec::new();
    let cleaned = spec.replace('，', ",").replace('－', "-").replace('–', "-");
    for part in cleaned.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let (a, b) = (a.trim(), b.trim());
            let from: u32 = a
                .parse()
                .map_err(|_| AppError::new("err.badPageSpec").var("got", part))?;
            // 「5-」表示从第 5 页到最后
            let to: u32 = if b.is_empty() {
                total
            } else {
                b.parse()
                    .map_err(|_| AppError::new("err.badPageSpec").var("got", part))?
            };
            let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
            out.extend(lo..=hi);
        } else {
            out.push(
                part.parse()
                    .map_err(|_| AppError::new("err.badPageSpec").var("got", part))?,
            );
        }
    }
    out.retain(|p| *p >= 1 && *p <= total);
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err(AppError::new("err.noPagesMatched"));
    }
    Ok(out)
}

/// 反转页序。扫描仪按倒序进纸是很常见的事故。
pub fn reverse_file(src: &Path) -> AppResult<(PathBuf, usize)> {
    let doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let n = pages.len();

    // 逐页抽成单页文档再倒着合并。直接改 Kids 数组更快，
    // 但页树里可能有嵌套节点，那样改容易把结构弄坏。
    let mut singles: Vec<Document> = Vec::with_capacity(n);
    let nums: Vec<u32> = pages.keys().copied().collect();
    for keep in nums.iter().rev() {
        let mut one = doc.clone();
        let drop: Vec<u32> = nums.iter().copied().filter(|p| p != keep).collect();
        one.delete_pages(&drop);
        one.adjust_zero_pages();
        singles.push(one);
    }

    let mut merged = merge_docs(singles)?;
    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 倒序", stem_of(src)), "pdf");
    save(&mut merged, &dst)?;
    Ok((dst, n))
}

fn pdf_reverse_blocking(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    run_batch(&app, paths, |src| {
        let (dst, n) = reverse_file(src)?;
        Ok((dst, Some(Note::new("note.pdfReversed").with("n", n))))
    })
}

/// 删除或保留指定页。
pub fn select_pages(src: &Path, spec: &str, keep_mode: bool) -> AppResult<(PathBuf, usize, usize)> {
    let mut doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let total = pages.len() as u32;
    let listed = parse_pages(spec, total)?;

    let all: Vec<u32> = pages.keys().copied().collect();
    let drop: Vec<u32> = if keep_mode {
        all.iter()
            .copied()
            .filter(|p| !listed.contains(p))
            .collect()
    } else {
        listed.clone()
    };

    if drop.len() >= all.len() {
        return Err(AppError::new("err.wouldDeleteAllPages"));
    }

    doc.delete_pages(&drop);
    doc.adjust_zero_pages();
    let left = all.len() - drop.len();

    let dir = output_dir_for(src)?;
    let label = if keep_mode { "保留页" } else { "删页" };
    let dst = unique_path(&dir, &format!("{} {label}", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, total as usize, left))
}

fn pdf_pages_blocking(
    app: AppHandle,
    paths: Vec<String>,
    spec: String,
    keep_mode: bool,
) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (dst, total, left) = select_pages(src, &spec, keep_mode)?;
        Ok((
            dst,
            Some(
                Note::new("note.pdfPages")
                    .with("total", total)
                    .with("left", left),
            ),
        ))
    })
}

// ================================================ 可视化整理页面（选/排/转）

/// 对某一页的一步操作：保留原文档里第 `page` 页（从 1 数），并额外旋转 `rotate` 度。
#[derive(serde::Deserialize)]
pub struct PageOp {
    /// 原文档里的页码，从 1 开始
    pub page: u32,
    /// 额外旋转的角度（0/90/180/270，累加到该页已有的 /Rotate 上）
    pub rotate: i64,
}

#[derive(serde::Serialize)]
pub struct ArrangeResult {
    pub out_path: String,
    pub pages: usize,
    /// 书签/表单等按页面引用的结构在整理中被移除了——告诉用户一声
    pub dropped: bool,
}

/// 把页面可继承的属性从旧 `/Pages` 祖先链解析出来。
///
/// PDF 规范允许 `MediaBox`、`CropBox`、`Resources`、`Rotate` 定义在 `/Pages`
/// 节点上、由下面的页面继承。真实 PDF 极常把 MediaBox 放在 Pages 上。
/// 一旦把页面的 `Parent` 换到新页树，这条继承链就断了——不先固化，页面就
/// 会丢尺寸（导出空白/错位）、丢资源（字体图片不显示）、丢原始旋转。
/// 返回「页面自身没有、但祖先有」的那些属性，交给调用方固化到页面上。
fn resolve_inherited(doc: &Document, page_id: ObjectId) -> Vec<(Vec<u8>, Object)> {
    let keys: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];
    let mut out: Vec<(Vec<u8>, Object)> = Vec::new();
    let Ok(page) = doc.get_object(page_id).and_then(|o| o.as_dict()) else {
        return out;
    };
    for &key in keys.iter() {
        if page.has(key) {
            continue;
        }
        let mut cur = page.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        let mut guard = 0;
        while let Some(pid) = cur {
            if guard > 64 {
                break; // 防环
            }
            guard += 1;
            let Ok(pd) = doc.get_object(pid).and_then(|o| o.as_dict()) else {
                break;
            };
            if let Ok(v) = pd.get(key) {
                out.push((key.to_vec(), v.clone()));
                break;
            }
            cur = pd.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        }
    }
    out
}

/// 按一份「保留哪些页、什么顺序、各转多少」的清单，重排出一份新 PDF。
///
/// 这一个函数同时覆盖了拆分/提取、删页、重排、逐页旋转——因为这些本质上
/// 都是「挑出若干原页、按新顺序、各带一个旋转」。前端的缩略图组织器把
/// 勾选、拖拽、转向都收敛成这份 `ops` 清单，后端照单执行即可。
///
/// **只打开文档一次、只改引用**：给每个选中的页重新挂到一个新建的扁平页树上
/// （顺序即 `ops` 顺序），旋转累加到它已有的 `/Rotate`，最后剪掉不再被引用的
/// 对象。早先的实现「每保留一页就克隆整份文档、最后合并」——4 页的测试全绿，
/// 但一份 200MB / 200 页的 PDF 会同时持有上百份文档克隆，内存可能爆到几十 GB。
/// 现在全程只有一份文档在内存里。
pub fn arrange_pages(src: &Path, ops: &[PageOp]) -> AppResult<(PathBuf, usize, bool)> {
    let mut doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    // 原文档页码升序；ops.page 是「第几页」，据此映射到真实页对象 id。
    let nums: Vec<u32> = pages.keys().copied().collect();
    let mut picked: Vec<(ObjectId, i64)> = Vec::new();
    for op in ops {
        if let Some(&pnum) = nums.get(op.page.saturating_sub(1) as usize) {
            if let Some(&id) = pages.get(&pnum) {
                picked.push((id, op.rotate));
            }
        }
    }
    if picked.is_empty() {
        return Err(AppError::new("err.pdfNoPagesPicked"));
    }
    let n = picked.len();

    // —— 只读阶段 ——
    // 换 Parent 会切断继承链，先把每个选中页可继承的属性解析出来。
    let inherited: Vec<Vec<(Vec<u8>, Object)>> = picked
        .iter()
        .map(|(id, _)| resolve_inherited(&doc, *id))
        .collect();
    // 保留原 Catalog 的文档级信息（XMP 元数据、语言、阅读器偏好等），别新建一个空的。
    let mut catalog = doc
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|o| o.as_reference().ok())
        .and_then(|id| doc.get_object(id).ok())
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_else(lopdf::Dictionary::new);

    // —— 可变阶段 ——
    // 新建一个扁平页树根：选中的页固化继承属性后重挂到它下面，旋转就地累加。
    let pages_id = doc.new_object_id();
    for ((id, rot), inh) in picked.iter().zip(inherited) {
        if let Ok(dict) = doc.get_object_mut(*id).and_then(|o| o.as_dict_mut()) {
            for (k, v) in inh {
                if !dict.has(&k) {
                    dict.set(k, v);
                }
            }
            dict.set("Parent", pages_id);
            if *rot != 0 {
                let cur = dict.get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0);
                dict.set("Rotate", ((cur + rot) % 360 + 360) % 360);
            }
        }
    }

    let kids: Vec<Object> = picked
        .iter()
        .map(|(id, _)| Object::Reference(*id))
        .collect();
    let mut pages_dict = lopdf::Dictionary::new();
    pages_dict.set("Type", "Pages");
    pages_dict.set("Count", n as u32);
    pages_dict.set("Kids", kids);
    doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

    catalog.set("Type", "Catalog");
    catalog.set("Pages", pages_id);
    // 这些都按页面引用，重排/删页后会指向已删的页、变成坏引用——明确摘掉。
    // 有则记一笔：整理动了页面，书签/表单这类没法无损带过来，如实告诉用户。
    let mut dropped = false;
    for k in [
        b"Outlines".as_ref(),
        b"AcroForm",
        b"Names",
        b"Dests",
        b"PageLabels",
        b"OpenAction",
        b"Threads",
        b"StructTreeRoot",
    ] {
        if catalog.remove(k).is_some() {
            dropped = true;
        }
    }
    let catalog_id = doc.add_object(Object::Dictionary(catalog));
    doc.trailer.set("Root", catalog_id);

    // 剪掉不再被 Root 触及的对象：旧页树、没选中的页、以及只被它们引用的资源。
    // 选中页仍需要的共享资源（字体/图片）因为还被引用，会保留。
    doc.prune_objects();
    doc.renumber_objects();
    doc.adjust_zero_pages();

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 整理", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, n, dropped))
}

#[tauri::command]
pub async fn pdf_arrange(path: String, ops: Vec<PageOp>) -> AppResult<ArrangeResult> {
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let src = PathBuf::from(&path);
        let (dst, n, dropped) = arrange_pages(&src, &ops)?;
        Ok::<ArrangeResult, AppError>(ArrangeResult {
            out_path: dst.to_string_lossy().to_string(),
            pages: n,
            dropped,
        })
    })
    .await;
    match joined {
        Ok(inner) => inner,
        Err(e) => Err(AppError::unknown(e)),
    }
}

// ================================================ 修复损坏的 PDF

/// 尽力抢救一份打不开的 PDF。
///
/// 实测 1070 份真实 PDF 里有 40 份解析失败。这类文件多半不是内容坏了，
/// 而是交叉引用表对不上——文件末尾那张「对象在第几字节」的索引失效了，
/// 严格的解析器于是拒绝整个文件，尽管页面数据完好地躺在里面。
///
/// 两条路依次试：
///
/// 1. **宽容加载后整份重写。** lopdf 能容忍一部分结构问题，读进来再
///    重新编号、重建索引写出去，索引就重新对上了。
/// 2. **交给系统渲染引擎。** 连 lopdf 都读不动时，用 `Windows.Data.Pdf`
///    逐页渲染成图再包回 PDF。**这一步会丢掉文字层**，产物变成扫描件，
///    所以只在第一条走不通时才用，而且必须如实告诉用户降级了——
///    悄悄把可搜索的文档换成一堆图片是不能接受的。
pub fn repair_file(src: &Path) -> AppResult<(PathBuf, usize, bool)> {
    let dir = output_dir_for(src)?;

    // 第一条路：读进来重写一遍
    if let Ok(mut doc) = Document::load(long_path(src)) {
        let pages = doc.get_pages().len();
        if pages > 0 && !doc.is_encrypted() {
            let dst = unique_path(&dir, &format!("{} 已修复", stem_of(src)), "pdf");
            save(&mut doc, &dst)?;
            // 写出来还得能再读回去，否则「修复」只是换了个坏法
            if let Ok(check) = Document::load(long_path(&dst)) {
                if check.get_pages().len() == pages {
                    return Ok((dst, pages, false));
                }
            }
        }
    }

    // 第二条路：渲染成图。丢文字层，但至少内容看得到。
    let count = crate::pdf_render::page_count(src)?;
    if count == 0 {
        return Err(AppError::new("err.repairFailed"));
    }

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    let tmp = std::env::temp_dir().join(format!("baobox_repair_{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;

    for i in 0..count {
        let Ok(png) = crate::pdf_render::render_page(src, i, 1600) else {
            continue;
        };
        let p = tmp.join(format!("r{i}.png"));
        std::fs::write(&p, &png)?;
        let Ok(image) = lopdf::xobject::image(long_path(&p)) else {
            continue;
        };
        let w = image
            .dict
            .get(b"Width")
            .and_then(|o| o.as_i64())
            .unwrap_or(1240) as f32;
        let h = image
            .dict
            .get(b"Height")
            .and_then(|o| o.as_i64())
            .unwrap_or(1754) as f32;
        // 按 A4 宽度换算成点，页面比例跟原件一致
        let pw = 595.0f32;
        let ph = pw * h / w.max(1.0);

        let mut pd = lopdf::Dictionary::new();
        pd.set("Type", "Page");
        pd.set("Parent", pages_id);
        pd.set(
            "MediaBox",
            vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(pw),
                Object::Real(ph),
            ],
        );
        let page = doc.add_object(Object::Dictionary(pd));
        doc.add_page_contents(page, Vec::new())
            .map_err(|e| AppError::unknown(e))?;
        doc.insert_image(page, image, (0.0, 0.0), (pw, ph))
            .map_err(|e| AppError::unknown(e))?;
        kids.push(Object::Reference(page));
        let _ = std::fs::remove_file(&p);
    }
    let _ = std::fs::remove_dir_all(&tmp);

    if kids.is_empty() {
        return Err(AppError::new("err.repairFailed"));
    }

    let n = kids.len();
    let mut tree = lopdf::Dictionary::new();
    tree.set("Type", "Pages");
    tree.set("Count", n as u32);
    tree.set("Kids", kids);
    doc.objects.insert(pages_id, Object::Dictionary(tree));
    let mut cat = lopdf::Dictionary::new();
    cat.set("Type", "Catalog");
    cat.set("Pages", pages_id);
    let catalog = doc.add_object(Object::Dictionary(cat));
    doc.trailer.set("Root", catalog);

    let dst = unique_path(&dir, &format!("{} 已修复（转为图片）", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, n, true))
}

fn pdf_repair_blocking(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    run_batch(&app, paths, |src| {
        let (dst, pages, rasterized) = repair_file(src)?;
        Ok((
            dst,
            Some(if rasterized {
                // 降级了就必须说，不能让用户以为拿回的还是原来那份可搜索文档
                Note::new("note.repairRaster").with("n", pages)
            } else {
                Note::new("note.repaired").with("n", pages)
            }),
        ))
    })
}

// ================================================ N 合 1 拼版

/// 把多页缩排到一页上。
///
/// 讲义、代码、合同草稿——打印出来大半是空白页边，2 合 1 直接省一半纸。
/// 靠 Form XObject 实现：把每一页原样包成一个可复用对象，再用变换矩阵
/// 缩放平移画上去。比重新排版内容可靠得多，页面里的字体、矢量、图片
/// 一律不用碰。
pub fn nup_file(src: &Path, cols: u32, rows: u32, gap: f32) -> AppResult<(PathBuf, usize, usize)> {
    let doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let per_sheet = (cols * rows) as usize;
    if per_sheet <= 1 {
        return Err(AppError::new("err.nupTooSmall"));
    }

    let src_ids: Vec<lopdf::ObjectId> = pages.values().copied().collect();
    let total = src_ids.len();

    // 以第一页的尺寸作为版面基准；混排尺寸的文档少见，
    // 真遇到也是按同一个格子放，不会画到格子外
    let (_, _, pw, ph) = page_box(&doc, src_ids[0]);

    let mut out = Document::with_version("1.5");
    let pages_id = out.new_object_id();
    let mut kids = Vec::new();
    let mut sheets = 0usize;

    for chunk in src_ids.chunks(per_sheet) {
        let mut content = String::new();
        let mut xobjects = lopdf::Dictionary::new();

        for (i, page_id) in chunk.iter().enumerate() {
            let Some(form_id) = page_to_form(&doc, &mut out, *page_id) else {
                continue;
            };
            let name = format!("BaoX{i}");
            xobjects.set(name.clone(), form_id);

            let (col, row) = (i as u32 % cols, i as u32 / cols);
            let cell_w = (pw - gap * (cols + 1) as f32) / cols as f32;
            let cell_h = (ph - gap * (rows + 1) as f32) / rows as f32;
            // 等比缩放，取两个方向里更紧的那个，保证整页装得下
            let scale = (cell_w / pw).min(cell_h / ph);
            let x = gap + col as f32 * (cell_w + gap) + (cell_w - pw * scale) / 2.0;
            // 第一格在左上，所以行号要从顶部往下数
            let y = ph - gap - (row + 1) as f32 * cell_h - row as f32 * gap
                + (cell_h - ph * scale) / 2.0;

            content.push_str(&format!(
                "q {scale:.5} 0 0 {scale:.5} {x:.2} {y:.2} cm /{name} Do Q\n"
            ));
        }

        let content_id = out.add_object(Object::Stream(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            content.into_bytes(),
        )));
        let mut res = lopdf::Dictionary::new();
        res.set("XObject", Object::Dictionary(xobjects));

        let mut page = lopdf::Dictionary::new();
        page.set("Type", "Page");
        page.set("Parent", pages_id);
        page.set("Contents", content_id);
        page.set("Resources", Object::Dictionary(res));
        page.set(
            "MediaBox",
            vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(pw),
                Object::Real(ph),
            ],
        );
        kids.push(Object::Reference(out.add_object(Object::Dictionary(page))));
        sheets += 1;
    }

    let mut pages_dict = lopdf::Dictionary::new();
    pages_dict.set("Type", "Pages");
    pages_dict.set("Count", kids.len() as u32);
    pages_dict.set("Kids", kids);
    out.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let mut cat = lopdf::Dictionary::new();
    cat.set("Type", "Catalog");
    cat.set("Pages", pages_id);
    let catalog = out.add_object(Object::Dictionary(cat));
    out.trailer.set("Root", catalog);

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} {}合1", stem_of(src), per_sheet), "pdf");
    save(&mut out, &dst)?;
    Ok((dst, total, sheets))
}

/// 把一页原样打包成可以画到别处的 Form XObject。
///
/// 页面的内容流、资源、以及它依赖的所有对象都得跟着搬到新文档里，
/// 漏一个引用产物就是空白页——而且不会报错。
fn page_to_form(
    src: &Document,
    out: &mut Document,
    page_id: lopdf::ObjectId,
) -> Option<lopdf::ObjectId> {
    let dict = src.get_object(page_id).ok()?.as_dict().ok()?;

    // 内容流可能是一个也可能是数组，拼成一份
    let mut content = Vec::new();
    match dict.get(b"Contents").ok()? {
        Object::Reference(r) => {
            if let Ok(Object::Stream(s)) = src.get_object(*r) {
                content.extend_from_slice(&s.get_plain_content().ok()?);
            }
        }
        Object::Array(a) => {
            for o in a {
                if let Object::Reference(r) = o {
                    if let Ok(Object::Stream(s)) = src.get_object(*r) {
                        content.extend_from_slice(&s.get_plain_content().unwrap_or_default());
                        content.push(b'\n');
                    }
                }
            }
        }
        _ => return None,
    }

    let resources = dict
        .get(b"Resources")
        .ok()
        .map(|r| deep_copy(src, out, r))
        .unwrap_or(Object::Dictionary(lopdf::Dictionary::new()));

    let (x, y, w, h) = page_box_of(src, page_id);
    let mut fd = lopdf::Dictionary::new();
    fd.set("Type", "XObject");
    fd.set("Subtype", "Form");
    fd.set("FormType", 1i64);
    fd.set(
        "BBox",
        vec![
            Object::Real(x),
            Object::Real(y),
            Object::Real(x + w),
            Object::Real(y + h),
        ],
    );
    fd.set("Resources", resources);

    let mut stream = lopdf::Stream::new(fd, content);
    let _ = stream.compress();
    Some(out.add_object(Object::Stream(stream)))
}

/// 把一个对象连同它引用到的一切复制到目标文档。
fn deep_copy(src: &Document, out: &mut Document, obj: &Object) -> Object {
    // 递归有环的风险：资源字典里出现自引用会栈溢出，限个深度
    fn go(src: &Document, out: &mut Document, obj: &Object, depth: u32) -> Object {
        if depth > 24 {
            return Object::Null;
        }
        match obj {
            Object::Reference(r) => match src.get_object(*r) {
                Ok(inner) => {
                    let copied = go(src, out, inner, depth + 1);
                    Object::Reference(out.add_object(copied))
                }
                Err(_) => Object::Null,
            },
            Object::Dictionary(d) => {
                let mut nd = lopdf::Dictionary::new();
                for (k, v) in d.iter() {
                    nd.set(k.to_vec(), go(src, out, v, depth + 1));
                }
                Object::Dictionary(nd)
            }
            Object::Array(a) => {
                Object::Array(a.iter().map(|v| go(src, out, v, depth + 1)).collect())
            }
            Object::Stream(s) => {
                let mut ns = s.clone();
                let mut nd = lopdf::Dictionary::new();
                for (k, v) in s.dict.iter() {
                    nd.set(k.to_vec(), go(src, out, v, depth + 1));
                }
                ns.dict = nd;
                Object::Stream(ns)
            }
            other => other.clone(),
        }
    }
    go(src, out, obj, 0)
}

/// page_box 的只读版本，用于跨文档取尺寸
fn page_box_of(doc: &Document, page_id: lopdf::ObjectId) -> (f32, f32, f32, f32) {
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
    let b = read(b"MediaBox").unwrap_or_else(|| vec![0.0, 0.0, 595.0, 842.0]);
    let (x0, y0, x1, y1) = (
        b[0].min(b[2]),
        b[1].min(b[3]),
        b[0].max(b[2]),
        b[1].max(b[3]),
    );
    (x0, y0, x1 - x0, y1 - y0)
}

fn pdf_nup_blocking(
    app: AppHandle,
    paths: Vec<String>,
    layout: String,
    gap: f32,
) -> Vec<FileOutcome> {
    let (cols, rows) = match layout.as_str() {
        "1x2" => (1u32, 2u32),
        "2x2" => (2, 2),
        "3x3" => (3, 3),
        "2x4" => (2, 4),
        _ => (2, 1),
    };
    run_batch(&app, paths, move |src| {
        let (dst, total, sheets) = nup_file(src, cols, rows, gap)?;
        Ok((
            dst,
            Some(
                Note::new("note.nup")
                    .with("total", total)
                    .with("sheets", sheets)
                    .with("per", cols * rows),
            ),
        ))
    })
}

// ================================================ 插入空白页

/// 在指定位置插入空白页。
///
/// 双面打印时常需要在章节之间补一张空白，让下一章从正面开始。
pub fn insert_blank(src: &Path, after_spec: &str, count: u32) -> AppResult<(PathBuf, usize)> {
    let mut doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let total = pages.len() as u32;
    // 空的话就在每一页后面都插，这正是「章节间补白」最常见的用法
    let targets: Vec<u32> = if after_spec.trim().is_empty() {
        (1..=total).collect()
    } else {
        parse_pages(after_spec, total)?
    };

    let ids: Vec<lopdf::ObjectId> = pages.values().copied().collect();
    let (_, _, pw, ph) = page_box(&doc, ids[0]);

    // 逐页拆成单页文档，在指定位置后插入空白，再合并回去
    let nums: Vec<u32> = pages.keys().copied().collect();
    let mut parts: Vec<Document> = Vec::new();
    let mut inserted = 0usize;

    for (i, n) in nums.iter().enumerate() {
        let mut one = doc.clone();
        let drop: Vec<u32> = nums.iter().copied().filter(|p| p != n).collect();
        one.delete_pages(&drop);
        one.adjust_zero_pages();
        parts.push(one);

        if targets.contains(&(i as u32 + 1)) {
            for _ in 0..count.max(1) {
                parts.push(blank_page(pw, ph));
                inserted += 1;
            }
        }
    }

    let mut merged = merge_docs(parts)?;
    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 加空白页", stem_of(src)), "pdf");
    save(&mut merged, &dst)?;
    let _ = &mut doc;
    Ok((dst, inserted))
}

fn blank_page(w: f32, h: f32) -> Document {
    let mut d = Document::with_version("1.5");
    let pages_id = d.new_object_id();
    let content = d.add_object(Object::Stream(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        Vec::new(),
    )));
    let mut pd = lopdf::Dictionary::new();
    pd.set("Type", "Page");
    pd.set("Parent", pages_id);
    pd.set("Contents", content);
    pd.set(
        "MediaBox",
        vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(w),
            Object::Real(h),
        ],
    );
    let page = d.add_object(Object::Dictionary(pd));

    let mut tree = lopdf::Dictionary::new();
    tree.set("Type", "Pages");
    tree.set("Count", 1i64);
    tree.set("Kids", vec![Object::Reference(page)]);
    d.objects.insert(pages_id, Object::Dictionary(tree));

    let mut cat = lopdf::Dictionary::new();
    cat.set("Type", "Catalog");
    cat.set("Pages", pages_id);
    let cat_id = d.add_object(Object::Dictionary(cat));
    d.trailer.set("Root", cat_id);
    d
}

fn pdf_blank_blocking(
    app: AppHandle,
    paths: Vec<String>,
    after: String,
    count: u32,
) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (dst, n) = insert_blank(src, &after, count)?;
        Ok((dst, Some(Note::new("note.blankInserted").with("n", n))))
    })
}

// ================================================ 元数据清除

/// PDF 里常见的身份字段。
///
/// 跟图片的 EXIF 是同一类问题，只是没人注意：Word 导出的 PDF 会把
/// 你的**电脑用户名**写进 /Author，扫描件里写着设备型号，投标文件里
/// 留着上一版的作者。发出去之前该清掉。
const META_KEYS: [&[u8]; 8] = [
    b"Title",
    b"Author",
    b"Subject",
    b"Keywords",
    b"Creator",
    b"Producer",
    b"CreationDate",
    b"ModDate",
];

/// 清掉文档信息与 XMP。返回产物路径和清掉的字段名（供界面如实告知）。
pub fn clean_metadata(src: &Path, keep_dates: bool) -> AppResult<(PathBuf, Vec<String>)> {
    let mut doc = open(src)?;
    let mut removed = Vec::new();

    // /Info 字典
    if let Ok(Object::Reference(info_id)) = doc.trailer.get(b"Info").cloned() {
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(info_id) {
            for key in META_KEYS {
                if keep_dates && (key == b"CreationDate" || key == b"ModDate") {
                    continue;
                }
                if let Ok(v) = d.get(key) {
                    // 空值不值得报告成「清掉了什么」
                    let text = match v {
                        Object::String(s, _) => String::from_utf8_lossy(s).trim().to_string(),
                        _ => String::new(),
                    };
                    if !text.is_empty() {
                        removed.push(String::from_utf8_lossy(key).to_string());
                    }
                    d.remove(key);
                }
            }
        }
    }

    // XMP 元数据流挂在 Catalog 上，是另一份独立的副本——
    // 只清 /Info 的话，用属性面板看着干净了，里面还留着一整份 XML。
    if let Ok(Object::Reference(root_id)) = doc.trailer.get(b"Root").cloned() {
        let had_xmp = doc
            .get_object(root_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .map(|d| d.get(b"Metadata").is_ok())
            .unwrap_or(false);
        if had_xmp {
            if let Ok(Object::Dictionary(d)) = doc.get_object_mut(root_id) {
                d.remove(b"Metadata");
            }
            // 只从 Catalog 摘掉引用还不够：XMP 流对象仍躺在对象表里，save 会把它
            // 原样写回。prune 掉现在不可达的对象，才真正把这份 XML 从产物里抹掉
            // （安全红线 7——界面承诺「XMP 那份独立副本也一起清」）。
            doc.prune_objects();
            removed.push("XMP".into());
        }
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 已清元数据", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, removed))
}

fn pdf_clean_meta_blocking(
    app: AppHandle,
    paths: Vec<String>,
    keep_dates: bool,
) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (dst, removed) = clean_metadata(src, keep_dates)?;
        Ok((
            dst,
            Some(if removed.is_empty() {
                Note::new("note.metaNone")
            } else {
                Note::new("note.metaCleaned")
                    .with("n", removed.len())
                    .with("list", removed.join(" / "))
            }),
        ))
    })
}

// ================================================ 裁掉页边空白

/// 自动裁掉页面四周的空白。
///
/// 扫描件和从网页导出的 PDF 常带着极宽的白边，打印出来正文只占中间一小块。
/// 靠渲染成图找内容边界——PDF 里的「内容在哪」没法从结构上直接读出来，
/// 文字、矢量、图片各有各的坐标系，渲染一遍反而是最可靠的。
///
/// 只改 CropBox，不动页面内容：裁错了把 CropBox 去掉就全回来了，
/// 而真删内容是不可逆的。
pub fn crop_file(src: &Path, margin_pt: f32) -> AppResult<(PathBuf, usize, usize)> {
    let mut doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }

    // 侦测用的分辨率不必高，找边界而已，低分辨率快得多
    const PROBE_W: u32 = 600;
    let mut cropped = 0usize;
    let total = pages.len();
    let ids: Vec<(u32, lopdf::ObjectId)> = pages.into_iter().collect();

    for (i, (_, page_id)) in ids.iter().enumerate() {
        let Ok(png) = crate::pdf_render::render_page(src, i as u32, PROBE_W) else {
            continue;
        };
        let Ok(img) = image::load_from_memory(&png) else {
            continue;
        };
        let Some((l, t, r, b)) = content_bounds(&img) else {
            // 整页空白，保持原样——裁成 0 尺寸只会让阅读器打不开
            continue;
        };

        let (bx, by, bw, bh) = page_box(&doc, *page_id);
        let sx = bw / img.width() as f32;
        let sy = bh / img.height() as f32;

        // 图的 y 向下，PDF 的 y 向上，上下边界要换过来
        let x0 = (bx + l as f32 * sx - margin_pt).max(bx);
        let x1 = (bx + (r + 1) as f32 * sx + margin_pt).min(bx + bw);
        let y0 = (by + bh - (b + 1) as f32 * sy - margin_pt).max(by);
        let y1 = (by + bh - t as f32 * sy + margin_pt).min(by + bh);

        // 收益太小就别改，免得产物页面尺寸参差不齐
        if (x1 - x0) > bw * 0.98 && (y1 - y0) > bh * 0.98 {
            continue;
        }
        if (x1 - x0) < 20.0 || (y1 - y0) < 20.0 {
            continue;
        }

        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(*page_id) {
            d.set(
                "CropBox",
                vec![
                    Object::Real(x0),
                    Object::Real(y0),
                    Object::Real(x1),
                    Object::Real(y1),
                ],
            );
        }
        cropped += 1;
    }

    if cropped == 0 {
        return Err(AppError::new("err.nothingToCrop"));
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 裁边", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, total, cropped))
}

/// 找出这一页上真正有内容的范围（像素坐标）
fn content_bounds(img: &image::DynamicImage) -> Option<(u32, u32, u32, u32)> {
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();
    // 扫描件的白底常带噪点，纯白判定会一点都裁不掉
    const WHITE: u8 = 245;

    let (mut l, mut t, mut r, mut b) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if gray.get_pixel(x, y).0[0] < WHITE {
                l = l.min(x);
                t = t.min(y);
                r = r.max(x);
                b = b.max(y);
            }
        }
    }
    (l != u32::MAX).then_some((l, t, r, b))
}

fn pdf_crop_blocking(app: AppHandle, paths: Vec<String>, margin: f32) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (dst, total, cropped) = crop_file(src, margin)?;
        Ok((
            dst,
            Some(
                Note::new("note.cropped")
                    .with("total", total)
                    .with("n", cropped),
            ),
        ))
    })
}

// ==================================================== 页码与水印

/// 取页面尺寸。MediaBox 可能带非零原点，直接当成宽高会把文字画到页面外。
fn page_box(doc: &Document, page_id: lopdf::ObjectId) -> (f32, f32, f32, f32) {
    let get = |key: &[u8]| -> Option<Vec<f32>> {
        let d = doc.get_object(page_id).ok()?.as_dict().ok()?;
        let arr = match d.get(key).ok()? {
            Object::Array(a) => a.clone(),
            Object::Reference(r) => doc.get_object(*r).ok()?.as_array().ok()?.clone(),
            _ => return None,
        };
        let v: Vec<f32> = arr
            .iter()
            .filter_map(|o| {
                o.as_f32()
                    .ok()
                    .or_else(|| o.as_i64().ok().map(|i| i as f32))
            })
            .collect();
        (v.len() == 4).then_some(v)
    };
    // 页面自己没有 MediaBox 时是从父节点继承的，这里退回 A4
    let b = get(b"MediaBox").unwrap_or_else(|| vec![0.0, 0.0, 595.0, 842.0]);
    (b[0], b[1], b[2], b[3])
}

pub struct StampOptions {
    /// 水印文字，空则不加水印
    pub watermark: String,
    /// 是否添加页码
    pub page_numbers: bool,
    /// 水印透明度 0–1
    pub opacity: f32,
}

pub fn stamp_file(src: &Path, opt: &StampOptions) -> AppResult<(PathBuf, usize, usize)> {
    let mut doc = open(src)?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(AppError::new("err.pdfNoPages"));
    }
    let total = pages.len();

    // 先把所有会用到的字符收集齐，一次性子集化。
    // 页码要用到的数字和「第页共」这几个字也得算进去。
    let mut needed = opt.watermark.clone();
    if opt.page_numbers {
        needed.push_str("第页共 0123456789");
        needed.push_str(&total.to_string());
    }
    let font = crate::pdf_font::prepare(&needed)?;
    let font_size_saved = font.source_bytes;
    let subset_size = font.data.len();

    let font_id = crate::pdf_font::embed(&mut doc, &font);
    let gs_id = (!opt.watermark.is_empty() && opt.opacity < 1.0)
        .then(|| crate::pdf_font::add_alpha_state(&mut doc, opt.opacity));

    let page_ids: Vec<(u32, lopdf::ObjectId)> = pages.into_iter().collect();
    for (page_no, page_id) in &page_ids {
        let (x0, y0, x1, y1) = page_box(&doc, *page_id);
        let (w, h) = (x1 - x0, y1 - y0);
        let mut ops = String::new();

        if !opt.watermark.is_empty() {
            let size =
                (w.min(h) / opt.watermark.chars().count().max(4) as f32 * 1.6).clamp(18.0, 72.0);
            let text_w = font.width_of_text(&opt.watermark, size);
            // 沿对角线居中放置，45 度
            let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
            let (cos, sin) = (0.7071_f32, 0.7071_f32);
            let (dx, dy) = (-text_w / 2.0, -size / 3.0);
            let (tx, ty) = (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos);
            ops.push_str("q\n");
            if gs_id.is_some() {
                ops.push_str("/BaoboxGS gs\n");
            }
            ops.push_str("0.45 0.45 0.45 rg\nBT\n");
            ops.push_str(&format!("/BaoboxF {size:.1} Tf\n"));
            ops.push_str(&format!(
                "{cos:.4} {sin:.4} {:.4} {cos:.4} {tx:.2} {ty:.2} Tm\n",
                -sin
            ));
            ops.push_str(&format!("<{}> Tj\nET\nQ\n", font.encode(&opt.watermark)));
        }

        if opt.page_numbers {
            let label = format!("第 {page_no} 页 共 {total} 页");
            let size = 10.0_f32;
            let tw = font.width_of_text(&label, size);
            let (tx, ty) = (x0 + (w - tw) / 2.0, y0 + 24.0);
            ops.push_str("q\n0.25 0.25 0.25 rg\nBT\n");
            ops.push_str(&format!("/BaoboxF {size:.1} Tf\n"));
            ops.push_str(&format!("1 0 0 1 {tx:.2} {ty:.2} Tm\n"));
            ops.push_str(&format!("<{}> Tj\nET\nQ\n", font.encode(&label)));
        }

        if !ops.is_empty() {
            crate::pdf_font::attach_resources(&mut doc, *page_id, font_id, gs_id);
            doc.add_page_contents(*page_id, ops.into_bytes())
                .map_err(|e| AppError::unknown(e))?;
        }
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 加标记", stem_of(src)), "pdf");
    save(&mut doc, &dst)?;
    Ok((dst, total, font_size_saved - subset_size))
}

fn pdf_stamp_blocking(
    app: AppHandle,
    paths: Vec<String>,
    text: String,
    page_numbers: bool,
    opacity: u8,
) -> Vec<FileOutcome> {
    let opt = StampOptions {
        watermark: text,
        page_numbers,
        opacity: (opacity as f32 / 100.0).clamp(0.05, 1.0),
    };
    run_batch(&app, paths, move |src| {
        let (dst, pages, _) = stamp_file(src, &opt)?;
        Ok((dst, Some(Note::new("note.stamped").with("pages", pages))))
    })
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
                let mut o = FileOutcome::ok(
                    &src,
                    dst,
                    Some(Note::new("note.extracted").with("chars", chars)),
                );
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
#[allow(non_snake_case)]
pub async fn pdf_extract_images(
    app: AppHandle,
    paths: Vec<String>,
    minPx: u32,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_extract_images_blocking(app, paths, minPx))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_repair(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_repair_blocking(app, paths))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_nup(
    app: AppHandle,
    paths: Vec<String>,
    layout: String,
    gap: u32,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        pdf_nup_blocking(app, paths, layout, gap.min(60) as f32)
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_blank(
    app: AppHandle,
    paths: Vec<String>,
    after: String,
    count: u32,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        pdf_blank_blocking(app, paths, after, count.clamp(1, 10))
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn pdf_clean_meta(
    app: AppHandle,
    paths: Vec<String>,
    keepDates: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_clean_meta_blocking(app, paths, keepDates))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_crop(app: AppHandle, paths: Vec<String>, margin: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        pdf_crop_blocking(app, paths, margin.min(72) as f32)
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_reverse(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_reverse_blocking(app, paths))
        .await
        .unwrap_or_default()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn pdf_pages(
    app: AppHandle,
    paths: Vec<String>,
    pages: String,
    keepMode: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_pages_blocking(app, paths, pages, keepMode))
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
pub async fn pdf_decrypt(app: AppHandle, paths: Vec<String>, password: String) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_decrypt_blocking(app, paths, password))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_compress(app: AppHandle, paths: Vec<String>, quality: u8) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_compress_blocking(app, paths, quality))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_stamp(
    app: AppHandle,
    paths: Vec<String>,
    text: String,
    page_numbers: bool,
    opacity: u8,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        pdf_stamp_blocking(app, paths, text, page_numbers, opacity)
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn pdf_to_text(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_to_text_blocking(app, paths))
        .await
        .unwrap_or_default()
}
