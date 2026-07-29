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
