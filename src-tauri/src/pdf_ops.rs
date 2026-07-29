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

    // 产物只有一份，挂在第一个输入上；其余每一个也要各自发一条「已并入」。
    // 早先只发第一条，界面上后面几行就永远停在「等待」，看着像处理到一半卡死了。
    let o = match result {
        Ok(dst) => FileOutcome::ok(
            &first,
            dst,
            Some(Note::new("note.merged").with("files", docs.len()).with("pages", count)),
        ),
        Err(e) => FileOutcome::fail(&first, e),
    };
    let rest: Vec<PathBuf> = docs.iter().skip(1).map(|(p, _)| p.clone()).collect();
    for (i, o) in crate::batch::fold_outcomes(o, &rest).into_iter().enumerate() {
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
            Some(Note::new("note.pdfRotate").with("n", n).with("deg", degrees)),
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
    stream.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
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

fn pdf_extract_images_blocking(app: AppHandle, paths: Vec<String>, min_px: u32) -> Vec<FileOutcome> {
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
            let from: u32 = a.parse().map_err(|_| AppError::new("err.badPageSpec").var("got", part))?;
            // 「5-」表示从第 5 页到最后
            let to: u32 = if b.is_empty() {
                total
            } else {
                b.parse().map_err(|_| AppError::new("err.badPageSpec").var("got", part))?
            };
            let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
            out.extend(lo..=hi);
        } else {
            out.push(part.parse().map_err(|_| AppError::new("err.badPageSpec").var("got", part))?);
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
        all.iter().copied().filter(|p| !listed.contains(p)).collect()
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
            Some(Note::new("note.pdfPages").with("total", total).with("left", left)),
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
            .filter_map(|o| o.as_f32().ok().or_else(|| o.as_i64().ok().map(|i| i as f32)))
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
            let size = (w.min(h) / opt.watermark.chars().count().max(4) as f32 * 1.6).clamp(18.0, 72.0);
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
            ops.push_str(&format!("{cos:.4} {sin:.4} {:.4} {cos:.4} {tx:.2} {ty:.2} Tm\n", -sin));
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
                let mut o =
                    FileOutcome::ok(&src, dst, Some(Note::new("note.extracted").with("chars", chars)));
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
pub async fn pdf_decrypt(
    app: AppHandle,
    paths: Vec<String>,
    password: String,
) -> Vec<FileOutcome> {
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
