//! 文本与文件类小工具。

use crate::batch::{run_batch, FileOutcome, Note};
use crate::err::{AppError, AppResult};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

// ================================================================ 乱码修复

/// 一份文本到底是什么编码。
///
/// 中文用户的老问题：GBK 存的 txt/csv 在 UTF-8 环境里打开是一堆「锟斤拷」，
/// 反过来也一样。欧美的文本工具基本不管这件事，因为他们很少撞上。
///
/// 用 chardetng（Firefox 检测器的 Rust 移植）判编码。它对 GBK / Big5 / Shift_JIS
/// 这类多字节编码的准确率明显高于「按字节猜」的土办法。
fn detect(bytes: &[u8]) -> &'static encoding_rs::Encoding {
    // BOM 是明示的，优先于任何统计判断
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return encoding_rs::UTF_8;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE;
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE;
    }
    let mut det = chardetng::EncodingDetector::new();
    det.feed(bytes, true);
    det.guess(None, true)
}

/// 按检测出的编码读成字符串。
///
/// 凡是要读用户文本文件的地方都该走这里——二维码生成、查找替换都一样。
/// 各写各的 from_utf8_lossy 的话，GBK 文件在别处照样是乱码，
/// 「乱码修复」就成了一个孤立的补丁而不是整个软件的默认行为。
pub fn decode_text(bytes: &[u8]) -> String {
    let (text, _, _) = detect(bytes).decode(bytes);
    text.into_owned()
}

/// 把文本转成 UTF-8。返回产物路径和检测到的原编码名。
pub fn fix_encoding(src: &Path, add_bom: bool) -> AppResult<(PathBuf, String, bool)> {
    let bytes = std::fs::read(long_path(src))?;
    if bytes.is_empty() {
        return Err(AppError::new("err.emptyFile"));
    }

    let enc = detect(&bytes);
    let (text, _, had_errors) = enc.decode(&bytes);

    // 已经是 UTF-8 且解码无损，就不必改了——如实说一声比默默复制一份好
    let already_utf8 = enc == encoding_rs::UTF_8 && !had_errors;

    let dir = output_dir_for(src)?;
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "txt".into());
    let dst = unique_path(&dir, &stem_of(src), &ext);

    let mut out = Vec::with_capacity(text.len() + 3);
    // 记事本和 Excel 靠 BOM 认 UTF-8；没有它，用 Excel 打开 CSV 又是一遍乱码。
    // 但给代码文件加 BOM 会让不少编译器报错，所以做成开关。
    if add_bom {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    out.extend_from_slice(text.as_bytes());
    std::fs::write(long_path(&dst), &out)?;

    Ok((dst, enc.name().to_string(), already_utf8))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn text_fix_encoding(
    app: AppHandle,
    paths: Vec<String>,
    addBom: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let (dst, from, already) = fix_encoding(src, addBom)?;
            Ok((
                dst,
                Some(if already {
                    Note::new("note.encAlready")
                } else {
                    Note::new("note.encFixed").with("from", from)
                }),
            ))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 简繁转换

/// 简体 ↔ 繁体。
///
/// 用 MediaWiki 的转换表，不是逐字映射。逐字表在中文里是会出错的：
/// 「头发」和「发展」共用一个「发」，繁体分别是「頭髮」和「發展」；
/// 「干」对应「幹 / 乾 / 干」三个。只有按词切分才判得对，
/// 而这正是维基百科多年维护那套表要解决的问题。
pub fn convert_zh(src: &Path, to_traditional: bool) -> AppResult<(PathBuf, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    if bytes.is_empty() {
        return Err(AppError::new("err.emptyFile"));
    }
    let text = decode_text(&bytes);

    let variant = if to_traditional {
        zhconv::Variant::ZhHant
    } else {
        zhconv::Variant::ZhHans
    };
    let out = zhconv::zhconv(&text, variant);

    // 一个字都没变通常意味着用户搞反了方向，说一声比默默复制一份有用
    let changed = out
        .chars()
        .zip(text.chars())
        .filter(|(a, b)| a != b)
        .count();

    let dir = output_dir_for(src)?;
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "txt".into());
    let dst = unique_path(&dir, &stem_of(src), &ext);
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(out.as_bytes());
    std::fs::write(long_path(&dst), &data)?;
    Ok((dst, changed))
}

#[tauri::command]
pub async fn text_zhconv(app: AppHandle, paths: Vec<String>, target: String) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let hant = target == "hant";
        run_batch(&app, paths, move |src| {
            let (dst, changed) = convert_zh(src, hant)?;
            Ok((
                dst,
                Some(if changed == 0 {
                    Note::new("note.zhNoChange")
                } else {
                    Note::new("note.zhConverted").with("n", changed)
                }),
            ))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 查找替换

/// 批量查找替换。
///
/// 编码走统一检测，所以一批 GBK 的老文件也能直接改，不用先跑一遍乱码修复。
/// 产物一律写新文件——安全红线 1，原文件绝不动。
pub fn replace_in_file(
    src: &Path,
    find: &str,
    replace: &str,
    use_regex: bool,
    case_sensitive: bool,
) -> AppResult<(PathBuf, usize)> {
    if find.is_empty() {
        return Err(AppError::new("err.emptyFind"));
    }
    let bytes = std::fs::read(long_path(src))?;
    let text = decode_text(&bytes);

    let (out, hits) = if use_regex {
        let re = regex::RegexBuilder::new(find)
            .case_insensitive(!case_sensitive)
            .build()
            .map_err(|e| AppError::new("err.badRegex").detail(e))?;
        let hits = re.find_iter(&text).count();
        (re.replace_all(&text, replace).into_owned(), hits)
    } else if case_sensitive {
        (text.replace(find, replace), text.matches(find).count())
    } else {
        // 不区分大小写的字面替换：把 find 转义后当正则用，
        // 免得自己写一遍大小写无关的搜索
        let re = regex::RegexBuilder::new(&regex::escape(find))
            .case_insensitive(true)
            .build()
            .map_err(|e| AppError::new("err.badRegex").detail(e))?;
        let hits = re.find_iter(&text).count();
        (re.replace_all(&text, replace).into_owned(), hits)
    };

    let dir = output_dir_for(src)?;
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "txt".into());
    let dst = unique_path(&dir, &stem_of(src), &ext);
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(out.as_bytes());
    std::fs::write(long_path(&dst), &data)?;
    Ok((dst, hits))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn text_replace(
    app: AppHandle,
    paths: Vec<String>,
    find: String,
    replace: String,
    useRegex: bool,
    caseSensitive: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let (dst, hits) = replace_in_file(src, &find, &replace, useRegex, caseSensitive)?;
            Ok((
                dst,
                Some(if hits == 0 {
                    Note::new("note.replaceNone")
                } else {
                    Note::new("note.replaced").with("n", hits)
                }),
            ))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 目录树导出

/// 把一个文件夹的结构导成纯文本树。
///
/// 用途很具体：交付时说明这个包里有什么、给同事讲项目结构、归档前留一份清单。
/// 手写不现实，截图又搜不了。
pub fn tree_of(root: &Path, max_depth: usize, show_size: bool) -> AppResult<String> {
    let mut out = String::new();
    out.push_str(&format!("{}\n", root.display()));
    walk(root, "", max_depth, 0, show_size, &mut out)?;
    Ok(out)
}

fn walk(
    dir: &Path,
    prefix: &str,
    max_depth: usize,
    depth: usize,
    show_size: bool,
    out: &mut String,
) -> AppResult<()> {
    if depth >= max_depth {
        return Ok(());
    }
    let mut entries: Vec<_> = match std::fs::read_dir(long_path(dir)) {
        Ok(rd) => rd.flatten().collect(),
        // 没权限的目录跳过就行，不该让整棵树导不出来
        Err(_) => return Ok(()),
    };
    // 目录在前、同类按名字排，输出才是稳定可比对的
    entries.sort_by_key(|e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        (!is_dir, e.file_name().to_string_lossy().to_lowercase())
    });

    let last = entries.len().saturating_sub(1);
    for (i, e) in entries.iter().enumerate() {
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let branch = if i == last { "└── " } else { "├── " };
        out.push_str(prefix);
        out.push_str(branch);
        out.push_str(&name);
        if is_dir {
            out.push('/');
        } else if show_size {
            if let Ok(m) = e.metadata() {
                out.push_str(&format!("  ({})", human_size(m.len())));
            }
        }
        out.push('\n');
        if is_dir {
            let next = format!("{prefix}{}", if i == last { "    " } else { "│   " });
            walk(&e.path(), &next, max_depth, depth + 1, show_size, out)?;
        }
    }
    Ok(())
}

fn human_size(b: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if b >= GB {
        format!("{:.2} GB", b as f64 / GB as f64)
    } else if b >= MB {
        format!("{:.1} MB", b as f64 / MB as f64)
    } else if b >= KB {
        format!("{} KB", b / KB)
    } else {
        format!("{b} B")
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn dir_tree(
    app: AppHandle,
    paths: Vec<String>,
    depth: u32,
    showSize: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let d = depth.clamp(1, 12) as usize;
        let total = paths.len();
        let mut out = Vec::with_capacity(total);
        crate::batch::reset_cancel();

        for (index, p) in paths.iter().enumerate() {
            let root = PathBuf::from(p);
            if crate::batch::cancelled() {
                let o = FileOutcome::skipped(&root, "run.cancelledSkip");
                crate::batch::emit(&app, index, total, &o);
                out.push(o);
                continue;
            }
            crate::batch::emit_working(&app, index, total, &crate::paths::file_name_of(&root));

            let o = match tree_of(&root, d, showSize) {
                Ok(text) => {
                    let lines = text.lines().count();
                    // 同时落一份 txt，方便直接发给别人
                    let saved = (|| -> AppResult<PathBuf> {
                        let dir = output_dir_for(&root)?;
                        let dst = unique_path(&dir, &format!("{} 目录树", stem_of(&root)), "txt");
                        let mut bytes = vec![0xEF, 0xBB, 0xBF];
                        bytes.extend_from_slice(text.as_bytes());
                        std::fs::write(long_path(&dst), &bytes)?;
                        Ok(dst)
                    })();
                    let mut o = FileOutcome::text_only(
                        &root,
                        text,
                        Some(Note::new("note.treeLines").with("n", lines)),
                    );
                    o.out_path = saved.ok().map(|d| d.to_string_lossy().to_string());
                    o
                }
                Err(e) => FileOutcome::fail(&root, e),
            };
            crate::batch::emit(&app, index, total, &o);
            out.push(o);
        }
        out
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 哈希校验

/// 算文件哈希。
///
/// 产物是文本不是文件——用户要的是能直接跟网站给的那串字对上，
/// 而不是又下载一个文件再打开。
pub fn hash_file(src: &Path, algo: &str) -> AppResult<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(long_path(src))?;
    // 分块读，别为了算个哈希把几 GB 的文件整个吞进内存
    let mut buf = vec![0u8; 1 << 20];

    match algo {
        "blake3" => {
            let mut h = blake3::Hasher::new();
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(h.finalize().to_hex().to_string())
        }
        _ => {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(format!("{:x}", h.finalize()))
        }
    }
}

#[tauri::command]
pub async fn file_hash(app: AppHandle, paths: Vec<String>, algo: String) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
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
            crate::batch::emit_working(&app, index, total, &crate::paths::file_name_of(&src));

            let o = match hash_file(&src, &algo) {
                Ok(hex) => FileOutcome::text_only(
                    &src,
                    hex,
                    Some(Note::new("note.hashAlgo").with("algo", algo.to_uppercase())),
                ),
                Err(e) => FileOutcome::fail(&src, e),
            };
            crate::batch::emit(&app, index, total, &o);
            out.push(o);
        }
        out
    })
    .await
    .unwrap_or_default()
}
