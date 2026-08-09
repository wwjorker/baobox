//! 批量解压。
//!
//! # 为什么值得专门做
//!
//! ZIP 规范里，如果通用标志位第 11 位没置上，条目名就是「本地代码页」的字节。
//! WinRAR 和早年的资源管理器打包时不置这一位，中文名于是以 GBK 存进去。
//! 到了另一台机器上，解压程序按 CP437 或 UTF-8 去读，得到的就是
//! 「鏂囦欢澶?」这类东西——**文件名彻底毁掉，而且没有任何提示**。
//!
//! Windows 自带的解压、以及大多数跨平台工具，都是这么坏的。
//! 这里的做法是：条目没声明 UTF-8 就拿原始字节自己判编码
//! （复用乱码修复那套检测器），判出 GBK / Big5 就按它解。
//!
//! # 另外两件必须做的事
//!
//! **防目录穿越。** 条目名可以写成 `..\..\Windows\System32\x.dll`。
//! 不拦的话解压一个恶意压缩包就能往系统目录写文件。每一条都要验，
//! 且是在**解码之后**验——只验原始字节会被编码技巧绕过。
//!
//! **自动建同名文件夹。** 一个包里几十个文件直接倒进当前目录是灾难。

use crate::batch::{fold_outcomes, FileOutcome, Note};
use crate::batch::run_batch;
use crate::err::{AppError, AppResult};
use crate::paths::{file_name_of, long_path, output_dir_for, stem_of, unique_path};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tauri::AppHandle;

// ================================================================ 打包成 zip

/// 把一批文件 / 文件夹压成一个 zip。与「解压」配对。
///
/// 这个工具收的是**原始拖入项**（文件夹不在前端展开），于是能保留结构：
///   · 拖一个文件夹进来 → 条目按「相对该文件夹的上级」算，**文件夹名本身
///     也进 zip**，里面的子目录层次原样保留。解开就是完整一个文件夹。
///   · 拖同一层的几个散文件 → 条目就是干净的文件名。
///
/// 早先的实现是把展开后的文件取「共同祖先」，有个洞：一个文件夹里若只有
/// `子目录/单个文件`，共同祖先会算到 `子目录`，连文件夹名带层次全丢了。
/// 直接按拖入项走就没这问题。
///
/// 名字一律用 UTF-8（zip 库默认置标志位第 11 位），中文名不会再变乱码——
/// 这正是我们解压那头在替别人收拾的烂摊子，自己产出时当然不留。
pub fn create_zip(srcs: &[PathBuf]) -> AppResult<(PathBuf, usize)> {
    // 遍历上限：误拖一个巨型目录（几十万文件）进来，不该让路径列表把内存吃光。
    // 内容本身是流式写的，这里限的只是「同时收集多少条路径」。
    const MAX_ENTRIES: usize = 50_000;
    // 收集 (zip 内条目名, 磁盘路径)
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let mut first_dir: Option<PathBuf> = None;
    let mut hit_cap = false;

    'outer: for src in srcs {
        let lp = long_path(src);
        if lp.is_dir() {
            if first_dir.is_none() {
                first_dir = Some(src.clone());
            }
            // 文件夹自己的名字作为 zip 内的顶层目录
            let folder = src.file_name().map(|n| n.to_string_lossy().to_string());
            for entry in jwalk::WalkDir::new(&lp).skip_hidden(false).into_iter().flatten() {
                if entries.len() >= MAX_ENTRIES {
                    hit_cap = true;
                    break 'outer;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let p = entry.path();
                // 别把上一轮自己的产物也打进去
                if p.components().any(|c| c.as_os_str() == crate::paths::OUTPUT_DIR) {
                    continue;
                }
                // 相对「被 walk 的根」算——两边都带 \\?\ 前缀，strip 才对得上；
                // 拿 src.parent()（无前缀）去 strip 会失败，把整条绝对路径塞进 zip。
                let within = p
                    .strip_prefix(&lp)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                let name = match &folder {
                    Some(f) if !f.is_empty() => format!("{f}/{within}"),
                    _ => within,
                };
                entries.push((name, p));
            }
        } else if lp.is_file() {
            if entries.len() >= MAX_ENTRIES {
                hit_cap = true;
                break;
            }
            entries.push((file_name_of(src), src.clone()));
        }
    }

    if hit_cap {
        // 宁可明确报错，也不悄悄只打包前一部分——那样用户会以为全打进去了。
        return Err(AppError::new("err.zipTooMany").var("max", MAX_ENTRIES.to_string()));
    }
    if entries.is_empty() {
        // 拖进来的全是空文件夹 / 没有真文件。这条现在真能到界面了：
        // 空文件夹会作为一行走到这里，而不是在前端被悄悄丢掉。
        return Err(AppError::new("err.zipNoFiles"));
    }

    let dir = output_dir_for(&srcs[0])?;
    // 只拖了一个文件夹就用它的名字；否则用第一项兜底
    let name = first_dir
        .as_ref()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| stem_of(&entries[0].1));
    let dst = unique_path(&dir, &format!("{name} 打包"), "zip");

    let out = std::fs::File::create(long_path(&dst))?;
    let mut zip = zip::ZipWriter::new(out);
    // deflate 是通用压缩法，几乎所有解压软件都认
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut n = 0usize;
    for (entry_name, path) in &entries {
        zip.start_file(entry_name.clone(), opts)
            .map_err(|e| AppError::unknown(e))?;
        // 流式拷贝，几个 GB 的大文件也不会一次性读进内存
        let mut f = std::fs::File::open(long_path(path))?;
        std::io::copy(&mut f, &mut zip)?;
        n += 1;
    }
    zip.finish().map_err(|e| AppError::unknown(e))?;
    Ok((dst, n))
}

#[tauri::command]
pub async fn zip_create(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        if paths.is_empty() {
            return Vec::new();
        }
        let srcs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let total = srcs.len();
        let o = match create_zip(&srcs) {
            Ok((dst, n)) => FileOutcome::ok(&srcs[0], dst, Some(Note::new("note.zipped").with("n", n))),
            Err(e) => FileOutcome::fail(&srcs[0], e),
        };
        // N→1：产物一份挂第一个，其余各发一条「已并入」，别让它们停在「等待」
        let outcomes = fold_outcomes(o, &srcs[1..]);
        for (i, o) in outcomes.iter().enumerate() {
            crate::batch::emit(&app, i, total, o);
        }
        outcomes
    })
    .await
    .unwrap_or_default()
}

/// 整个压缩包用哪个编码存文件名。
///
/// **必须整包一起判，不能一条一条判。** 一个文件名只有几个非 ASCII 字节，
/// 统计检测器在这么短的输入上极不可靠——实测「报告.txt」（4 个 GBK 字节）
/// 会被判成 Big5，解出「惆豢.txt」。而同一个包里所有名字拼起来就有足够信号，
/// 而且一个包本来就只有一种编码，逐条判还会出现同一包内前后不一致。
///
/// 先试系统的 ANSI 代码页。理由很直接：会遇到这个问题的人，用的机器
/// 通常跟当初打包那台是同一个地区设置——中文 Windows 上就是 GBK，
/// 而这正是这些包里的编码。系统代码页解得干净就用它，否则再交给检测器。
fn pick_encoding(names: &[Vec<u8>]) -> Option<&'static encoding_rs::Encoding> {
    let non_ascii: Vec<&Vec<u8>> = names.iter().filter(|n| !n.is_ascii()).collect();
    if non_ascii.is_empty() {
        return None;
    }
    // 全都是合法 UTF-8 就不用改。GBK 字节序列几乎不可能同时是合法 UTF-8，
    // 所以这条判断在实际文件上足够可靠。
    if non_ascii.iter().all(|n| std::str::from_utf8(n).is_ok()) {
        return None;
    }

    let mut joined = Vec::new();
    for n in &non_ascii {
        joined.extend_from_slice(n);
        joined.push(b'\n');
    }

    if let Some(acp) = system_codepage_encoding() {
        let (_, _, had_errors) = acp.decode(&joined);
        if !had_errors {
            return Some(acp);
        }
    }

    let mut det = chardetng::EncodingDetector::new();
    det.feed(&joined, true);
    Some(det.guess(None, true))
}

/// 系统 ANSI 代码页对应的编码。
#[cfg(windows)]
fn system_codepage_encoding() -> Option<&'static encoding_rs::Encoding> {
    let cp = unsafe { windows::Win32::Globalization::GetACP() };
    match cp {
        // GB18030 是 GBK 的超集，用它解 GBK 内容结果一致且覆盖更全
        936 => Some(encoding_rs::GB18030),
        950 => Some(encoding_rs::BIG5),
        932 => Some(encoding_rs::SHIFT_JIS),
        949 => Some(encoding_rs::EUC_KR),
        _ => None,
    }
}

#[cfg(not(windows))]
fn system_codepage_encoding() -> Option<&'static encoding_rs::Encoding> {
    None
}

/// 按定好的编码解一条名字。返回 (名字, 是否动用了编码修复)——
/// 「我们猜了一个编码」和「包里本来就写对了」是两回事，要如实报告。
fn decode_name(raw: &[u8], enc: Option<&'static encoding_rs::Encoding>) -> (String, bool) {
    if raw.is_ascii() {
        return (String::from_utf8_lossy(raw).to_string(), false);
    }
    match enc {
        Some(e) => {
            let (text, _, _) = e.decode(raw);
            (text.into_owned(), true)
        }
        None => (String::from_utf8_lossy(raw).to_string(), false),
    }
}

/// 把条目名清成一个安全的相对路径。
///
/// 拒绝绝对路径、盘符、以及任何 `..`。返回 None 表示这一条不该解。
fn safe_relative(name: &str) -> Option<PathBuf> {
    // zip 里一律是正斜杠，但见过反斜杠的实现，两种都拆
    let normalized = name.replace('\\', "/");
    let mut out = PathBuf::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        // Windows 上这些字符不能出现在文件名里，出现了就换掉，
        // 否则整条解不出来
        let cleaned: String = part
            .chars()
            .map(|c| if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
            .collect();
        if cleaned.trim().is_empty() {
            return None;
        }
        out.push(cleaned);
    }
    // 再兜一层：拼完之后不许出现任何非普通段
    if out
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

pub struct ExtractReport {
    pub dir: PathBuf,
    pub files: usize,
    /// 名字被编码修复过的条目数
    pub fixed_names: usize,
    /// 因为路径不安全而拒解的条目数
    pub rejected: usize,
    /// 因为压缩方法不支持而跳过的条目数
    pub unsupported: usize,
}

pub fn extract(src: &Path, password: Option<&str>) -> AppResult<ExtractReport> {
    // 防解压炸弹的三道闸：单条上限、累计上限、条目数上限。
    // 原来用 read_to_end 会把整条解压进内存，一个高压缩比的小包就能撑爆内存；
    // 改成流式写 + 这三个上限挡住。
    const MAX_ENTRY_BYTES: u64 = 2u64 << 30; // 单条最多解出 2 GiB
    const MAX_TOTAL_BYTES: u64 = 8u64 << 30; // 整包累计最多解出 8 GiB
    const MAX_ENTRIES: usize = 100_000; // 条目数上限，挡「百万空条目」

    let file = std::fs::File::open(long_path(src))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::new("err.badArchive").detail(e))?;
    if zip.len() > MAX_ENTRIES {
        return Err(AppError::new("err.archiveTooLarge"));
    }

    // 自动建同名文件夹：一个包里几十个文件倒进同一层是灾难
    let base = output_dir_for(src)?.join(stem_of(src));
    std::fs::create_dir_all(long_path(&base))?;

    let mut rep = ExtractReport {
        dir: base.clone(),
        files: 0,
        fixed_names: 0,
        rejected: 0,
        unsupported: 0,
    };

    // 先把所有名字收齐，整包判一次编码。见 pick_encoding 的说明——
    // 逐条判在文件名这种短输入上不可靠，而且会出现同包内前后不一致。
    let mut raw_names = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        match zip.by_index_raw(i) {
            Ok(e) => raw_names.push(e.name_raw().to_vec()),
            Err(_) => raw_names.push(Vec::new()),
        }
    }
    let enc = pick_encoding(&raw_names);

    let mut total_out: u64 = 0;
    for i in 0..zip.len() {
        // 加密条目在取的时候就会失败，逐条容错而不是整包放弃
        let entry = match password {
            Some(p) if !p.is_empty() => match zip.by_index_decrypt(i, p.as_bytes()) {
                Ok(e) => e,
                Err(_) => {
                    rep.unsupported += 1;
                    continue;
                }
            },
            _ => match zip.by_index(i) {
                Ok(e) => e,
                Err(_) => {
                    rep.unsupported += 1;
                    continue;
                }
            },
        };

        let raw = entry.name_raw().to_vec();
        let (name, fixed) = decode_name(&raw, enc);

        let Some(rel) = safe_relative(&name) else {
            rep.rejected += 1;
            continue;
        };
        let dst = base.join(&rel);

        if entry.is_dir() {
            std::fs::create_dir_all(long_path(&dst))?;
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(long_path(parent))?;
        }

        // 声明尺寸就超单条上限：直接拒，连写句柄都不开
        if entry.size() > MAX_ENTRY_BYTES {
            rep.rejected += 1;
            continue;
        }
        // 加上这条会超累计上限：拒掉，但继续看后面的小条目
        if total_out.saturating_add(entry.size()) > MAX_TOTAL_BYTES {
            rep.rejected += 1;
            continue;
        }
        let mut out = match std::fs::File::create(long_path(&dst)) {
            Ok(f) => f,
            Err(_) => {
                rep.unsupported += 1;
                continue;
            }
        };
        // 流式写，不把整条读进内存。take 是第二道保险：哪怕 size 字段撒谎，
        // 最多也只读到上限 +1 字节就判为炸弹。
        let mut capped = entry.take(MAX_ENTRY_BYTES + 1);
        match std::io::copy(&mut capped, &mut out) {
            Ok(n) if n > MAX_ENTRY_BYTES => {
                // 实际解压超上限（声明尺寸撒谎的炸弹）——删半成品、判拒绝
                drop(out);
                let _ = std::fs::remove_file(long_path(&dst));
                rep.rejected += 1;
            }
            Ok(n) => {
                total_out += n;
                rep.files += 1;
                if fixed {
                    rep.fixed_names += 1;
                }
            }
            Err(_) => {
                // 不支持的压缩方法（lzma / bzip2 等）会在这里失败，清理半成品
                drop(out);
                let _ = std::fs::remove_file(long_path(&dst));
                rep.unsupported += 1;
            }
        }
    }

    if rep.files == 0 && rep.unsupported > 0 {
        return Err(AppError::new("err.archiveUnsupported"));
    }
    if rep.files == 0 {
        return Err(AppError::new("err.archiveEmpty"));
    }
    Ok(rep)
}

#[tauri::command]
pub async fn zip_extract(
    app: AppHandle,
    paths: Vec<String>,
    password: String,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let pw = (!password.is_empty()).then_some(password.as_str());
            let rep = extract(src, pw)?;
            let mut note = Note::new("note.unzipped").with("n", rep.files);
            // 这三件事都得说出来。默默修掉编码、默默跳过条目，
            // 用户会以为解出来的就是全部。
            if rep.fixed_names > 0 {
                note = note.plus("note.unzipFixedNames").with("n", rep.fixed_names);
            }
            if rep.rejected > 0 {
                note = note.plus("note.unzipRejected").with("n", rep.rejected);
            }
            if rep.unsupported > 0 {
                note = note.plus("note.unzipSkipped").with("n", rep.unsupported);
            }
            Ok((rep.dir, Some(note)))
        })
    })
    .await
    .unwrap_or_default()
}
