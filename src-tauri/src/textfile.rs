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
/// 用 OpenCC 的按词转换表，不是逐字映射。逐字表在中文里是会出错的：
/// 「头发」和「发展」共用一个「发」，繁体分别是「頭髮」和「發展」；
/// 「干」对应「幹 / 乾 / 干」三个。只有按词切分才判得对，
/// 而这正是 OpenCC 那套词典要解决的问题。
///
/// 传入已建好的转换器：`OpenCC::new()` 要解析并建索引整套词典，
/// 每个文件重建一次是几十毫秒的浪费，一批共用一个即可。
pub fn convert_zh(
    cc: &opencc_fmmseg::OpenCC,
    src: &Path,
    to_traditional: bool,
) -> AppResult<(PathBuf, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    if bytes.is_empty() {
        return Err(AppError::new("err.emptyFile"));
    }
    let text = decode_text(&bytes);

    // s2t / t2s 是 OpenCC 的配置名：简→繁 / 繁→简。标点不转，
    // 中文用户的文本里全角标点本来就在用，强行换成另一套反而添乱。
    let config = if to_traditional { "s2t" } else { "t2s" };
    let out = cc.convert(&text, config, false);

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
        let cc = opencc_fmmseg::OpenCC::new();
        run_batch(&app, paths, move |src| {
            let (dst, changed) = convert_zh(&cc, src, hant)?;
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

// ================================================================ 大文件分割与合并

/// 分割成固定大小的分卷。
///
/// 邮件附件上限、网盘单文件上限、U 盘的 FAT32 四 GB 上限——
/// 这几个场景至今没消失。命名用 `.001 .002`，跟常见压缩软件一致，
/// 别人拿到手知道怎么处理。
pub fn split_file(src: &Path, part_mb: u64) -> AppResult<(PathBuf, usize)> {
    use std::io::{Read, Write};

    let size = std::fs::metadata(long_path(src))?.len();
    let chunk = part_mb.max(1) * 1024 * 1024;
    if size <= chunk {
        return Err(AppError::new("err.smallerThanPart"));
    }

    let dir = output_dir_for(src)?;
    let name = crate::paths::file_name_of(src);
    let mut f = std::fs::File::open(long_path(src))?;
    let mut buf = vec![0u8; 1 << 20];
    let mut part = 0usize;
    let mut last;

    loop {
        part += 1;
        // 分卷名保留原文件的完整名字（含扩展名），合并时才拼得回去
        let dst = dir.join(format!("{name}.{part:03}"));
        let mut out = std::fs::File::create(long_path(&dst))?;
        let mut written = 0u64;
        while written < chunk {
            let want = ((chunk - written) as usize).min(buf.len());
            let n = f.read(&mut buf[..want])?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            written += n as u64;
        }
        out.flush()?;
        last = dst;
        if written == 0 {
            // 正好整除时最后会多建一个空分卷，删掉
            let _ = std::fs::remove_file(long_path(&last));
            part -= 1;
            break;
        }
        if written < chunk {
            break;
        }
    }
    Ok((last, part))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn file_split(app: AppHandle, paths: Vec<String>, partMb: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let mb = partMb.max(1) as u64;
        run_batch(&app, paths, move |src| {
            let (last, n) = split_file(src, mb)?;
            Ok((last, Some(Note::new("note.splitParts").with("n", n).with("mb", mb))))
        })
    })
    .await
    .unwrap_or_default()
}

/// 把分卷拼回去。传入第一卷（`.001`），自动往后找。
pub fn join_file(first: &Path) -> AppResult<(PathBuf, usize, u64)> {
    use std::io::Write;

    let name = crate::paths::file_name_of(first);
    // 必须以 .001 结尾——从中间某一卷开始拼出来的是残文件，
    // 而且同样打得开，用户不会立刻发现
    let Some(stem) = name.strip_suffix(".001") else {
        return Err(AppError::new("err.notFirstPart"));
    };
    let parent = first.parent().unwrap_or_else(|| Path::new("."));

    let dir = output_dir_for(first)?;
    let dst = unique_path(
        &dir,
        Path::new(stem)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| stem.to_string())
            .as_str(),
        &Path::new(stem)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_else(|| "bin".into()),
    );

    let mut out = std::fs::File::create(long_path(&dst))?;
    let mut n = 0usize;
    let mut total = 0u64;
    loop {
        let part = parent.join(format!("{stem}.{:03}", n + 1));
        if !long_path(&part).exists() {
            break;
        }
        let data = std::fs::read(long_path(&part))?;
        out.write_all(&data)?;
        total += data.len() as u64;
        n += 1;
    }
    out.flush()?;

    if n == 0 {
        let _ = std::fs::remove_file(long_path(&dst));
        return Err(AppError::new("err.notFirstPart"));
    }
    Ok((dst, n, total))
}

#[tauri::command]
pub async fn file_join(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, |src| {
            let (dst, n, _) = join_file(src)?;
            Ok((dst, Some(Note::new("note.joinedParts").with("n", n))))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 按行处理

/// 去重 / 排序 / 词频。
///
/// 三件事共用一个工具：拿到一份名单或日志要做的往往就是这几步的组合，
/// 拆成三个工具等于让人跑三遍、存三份中间文件。
pub fn process_lines(
    src: &Path,
    dedupe: bool,
    sort: bool,
    count: bool,
) -> AppResult<(PathBuf, usize, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    let text = decode_text(&bytes);
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    let before = lines.len();

    let out_text = if count {
        // 词频模式：按出现次数排，同频按字母序，输出「次数<TAB>内容」
        let mut freq: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for l in lines.iter().filter(|l| !l.trim().is_empty()) {
            *freq.entry(l).or_insert(0) += 1;
        }
        let mut pairs: Vec<(&str, usize)> = freq.into_iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        pairs
            .iter()
            .map(|(l, c)| format!("{c}\t{l}"))
            .collect::<Vec<_>>()
            .join("\r\n")
    } else {
        let mut work: Vec<&str> = lines.clone();
        if dedupe {
            let mut seen = std::collections::HashSet::new();
            work.retain(|l| seen.insert(*l));
        }
        if sort {
            work.sort_unstable();
        }
        work.join("\r\n")
    };

    let after = out_text.lines().count();
    let dir = output_dir_for(src)?;
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "txt".into());
    let dst = unique_path(&dir, &stem_of(src), &ext);
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(out_text.as_bytes());
    std::fs::write(long_path(&dst), &data)?;
    Ok((dst, before, after))
}

#[tauri::command]
pub async fn text_lines(
    app: AppHandle,
    paths: Vec<String>,
    dedupe: bool,
    sort: bool,
    count: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let (dst, before, after) = process_lines(src, dedupe, sort, count)?;
            Ok((
                dst,
                Some(if count {
                    Note::new("note.lineFreq").with("n", after)
                } else {
                    Note::new("note.lineResult").with("before", before).with("after", after)
                }),
            ))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ CSV ↔ JSON

/// 表格和 JSON 互转。
///
/// 编码走统一检测，所以 Excel 导出的 GBK 的 CSV 也能直接吃。
/// 自己解析而不是拉一个 csv 库：需要的只是「引号、逗号、换行」三条规则，
/// 而且要能容忍不规范的文件——真实世界的 CSV 极少严格合规。
pub fn csv_to_json(src: &Path, pretty: bool) -> AppResult<(PathBuf, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    let text = decode_text(&bytes);
    let rows = parse_csv(&text);
    if rows.len() < 2 {
        return Err(AppError::new("err.csvNoRows"));
    }

    let head = &rows[0];
    let items: Vec<serde_json::Value> = rows[1..]
        .iter()
        .filter(|r| r.iter().any(|c| !c.trim().is_empty()))
        .map(|r| {
            let mut m = serde_json::Map::new();
            for (i, key) in head.iter().enumerate() {
                let k = if key.trim().is_empty() {
                    format!("列{}", i + 1)
                } else {
                    key.clone()
                };
                m.insert(k, serde_json::Value::String(r.get(i).cloned().unwrap_or_default()));
            }
            serde_json::Value::Object(m)
        })
        .collect();

    let n = items.len();
    let value = serde_json::Value::Array(items);
    let out = if pretty {
        serde_json::to_string_pretty(&value)
    } else {
        serde_json::to_string(&value)
    }
    .map_err(|e| AppError::unknown(e))?;

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &stem_of(src), "json");
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(out.as_bytes());
    std::fs::write(long_path(&dst), &data)?;
    Ok((dst, n))
}

/// JSON 数组转 CSV。列取所有对象键的并集，保持首次出现的顺序。
pub fn json_to_csv(src: &Path) -> AppResult<(PathBuf, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    let text = decode_text(&bytes);
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| AppError::new("err.badJson").detail(e))?;
    let serde_json::Value::Array(items) = value else {
        return Err(AppError::new("err.jsonNotArray"));
    };
    if items.is_empty() {
        return Err(AppError::new("err.csvNoRows"));
    }

    let mut cols: Vec<String> = Vec::new();
    for it in &items {
        if let serde_json::Value::Object(m) = it {
            for k in m.keys() {
                if !cols.contains(k) {
                    cols.push(k.clone());
                }
            }
        }
    }
    if cols.is_empty() {
        return Err(AppError::new("err.jsonNotArray"));
    }

    let mut out = String::new();
    out.push_str(&cols.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
    out.push_str("\r\n");
    for it in &items {
        let row: Vec<String> = cols
            .iter()
            .map(|c| {
                let v = it.get(c);
                csv_cell(&match v {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Null) | None => String::new(),
                    Some(other) => other.to_string(),
                })
            })
            .collect();
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &stem_of(src), "csv");
    // Excel 靠 BOM 认 UTF-8，没有它中文列名又是一遍乱码
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(out.as_bytes());
    std::fs::write(long_path(&dst), &data)?;
    Ok((dst, items.len()))
}

/// 含逗号、引号或换行的单元格要用引号包起来，内部的引号双写
fn csv_cell(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 手写 CSV 解析：认引号包裹、双写转义、字段内换行。
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut cell)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

#[tauri::command]
pub async fn data_convert(
    app: AppHandle,
    paths: Vec<String>,
    pretty: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            // 按扩展名决定方向，用户不用再选一次——CSV 只可能转 JSON，反之亦然
            let is_json = src
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase() == "json")
                .unwrap_or(false);
            if is_json {
                let (dst, n) = json_to_csv(src)?;
                Ok((dst, Some(Note::new("note.toCsv").with("n", n))))
            } else {
                let (dst, n) = csv_to_json(src, pretty)?;
                Ok((dst, Some(Note::new("note.toJson").with("n", n))))
            }
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 修改时间戳

/// 批量改文件时间。
///
/// 相机时区设错、导出工具把时间全写成当下、扫描件按处理顺序而不是
/// 拍摄顺序排——照片按时间排序全乱掉，这是唯一的修法。
///
/// **这个工具会直接改原文件的时间属性**，因为「时间」不是内容，
/// 复制一份出来改时间毫无意义。内容一个字节都不动。
pub fn set_times(src: &Path, shift_hours: i64, set_to: Option<i64>) -> AppResult<i64> {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let meta = std::fs::metadata(long_path(src))?;
    let current = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let target = match set_to {
        Some(ts) => ts,
        None => current + shift_hours * 3600,
    };
    if target < 0 {
        return Err(AppError::new("err.timeBeforeEpoch"));
    }

    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(target as u64);
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(long_path(src))?;
    f.set_modified(t)?;
    Ok(target)
}

/// 读文件当前的修改时间（Unix 秒）。撤销全靠先把它记下来。
fn read_mtime(src: &Path) -> Option<i64> {
    use std::time::UNIX_EPOCH;
    std::fs::metadata(long_path(src))
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

#[derive(serde::Serialize)]
pub struct TouchResult {
    pub done: usize,
    pub failed: usize,
    /// 撤销日志位置；为空表示没能存下（则界面不提供撤销）
    pub undo_log: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TouchUndoEntry {
    path: String,
    /// 原来的修改时间（Unix 秒）
    secs: i64,
}

/// 批量改文件的修改时间，并写一份可撤销的日志。
///
/// 这个工具会**直接改原文件**的时间属性——时间不是内容，复制一份出来改
/// 毫无意义。正因为动了原件，就得像重命名一样留后路：动手前先把每个文件的
/// 原时间记下来，撤销即可一键回到从前。**日志必须先能落盘才允许改第一个**，
/// 建不起来（磁盘满 / 无权限）就直接报错、一个都不动。
///
/// 走**写前日志**：先把每个文件的原时间都读出来、整份写进日志并 `sync_all`
/// 确认落盘，成功了才开始改。读时间和写日志都在动手之前完成，所以「改到一半
/// 磁盘写满、日志缺项」这种情况从结构上就不会发生。日志写不进去就报错、一个不动。
#[tauri::command]
#[allow(non_snake_case)]
pub fn touch_apply(paths: Vec<String>, shiftHours: i64) -> AppResult<TouchResult> {
    // 先把能读到原时间的文件收成完整计划。读不到时间的（无法 stat）后面也改不了。
    let plan: Vec<TouchUndoEntry> = paths
        .iter()
        .filter_map(|p| {
            let src = PathBuf::from(p);
            read_mtime(&src).map(|secs| TouchUndoEntry {
                path: src.to_string_lossy().to_string(),
                secs,
            })
        })
        .collect();
    let unreadable = paths.len() - plan.len();

    if plan.is_empty() {
        return Ok(TouchResult {
            done: 0,
            failed: unreadable,
            undo_log: String::new(),
        });
    }

    let sidecar_dir = paths
        .first()
        .map(PathBuf::from)
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    // 复用重命名那套写前日志：整份计划先落盘 + sync_all，成功才动手
    let log_path = crate::rename::write_ahead_log(
        sidecar_dir.as_deref(),
        &format!("Baobox 改时间撤销 {}.jsonl", touch_stamp()),
        &plan,
    )?;

    let (mut done, mut failed) = (0usize, 0usize);
    for e in &plan {
        match set_times(Path::new(&e.path), shiftHours, None) {
            Ok(_) => done += 1,
            Err(_) => failed += 1,
        }
    }

    Ok(TouchResult {
        done,
        failed: failed + unreadable,
        undo_log: log_path.to_string_lossy().to_string(),
    })
}

/// 按日志把时间改回去。用 set_to 设回原来的精确时刻。
/// 只有「一条都没失败」才删日志；有失败（含解析不了的坏行）就留着可再试，
/// 前端据 `failed > 0` 保留撤销按钮。
#[tauri::command]
pub fn touch_undo(log_path: String) -> AppResult<crate::rename::UndoResult> {
    let data = std::fs::read_to_string(long_path(Path::new(&log_path)))?;
    let mut entries: Vec<TouchUndoEntry> = Vec::new();
    let mut failed = 0usize; // 解析不了的行先记为失败
    for line in data.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<TouchUndoEntry>(line) {
            Ok(e) => entries.push(e),
            Err(_) => failed += 1,
        }
    }
    let mut restored = 0usize;
    for e in &entries {
        if set_times(Path::new(&e.path), 0, Some(e.secs)).is_ok() {
            restored += 1;
        } else {
            failed += 1;
        }
    }
    if failed == 0 {
        let _ = std::fs::remove_file(long_path(Path::new(&log_path)));
    }
    Ok(crate::rename::UndoResult { restored, failed })
}

/// 复用重命名那套纳秒时间戳，避免同一秒内撞名盖掉上一份日志。
fn touch_stamp() -> String {
    crate::rename::chrono_stamp()
}

// ================================================================ 批量新建文件夹

/// 按一份清单批量建文件夹。
///
/// 开学建三十个学生目录、按月份建十二个归档目录——手点三十次没人乐意。
/// 支持一层嵌套（`2026/01` 这种写法），但不许 `..`，
/// 否则一份清单就能往上级目录乱建。
pub fn make_dirs(src: &Path) -> AppResult<(PathBuf, usize, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    let text = decode_text(&bytes);

    // 建在源清单旁边，而不是输出目录里——建目录的意图通常是「就地组织」
    let base = src.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let mut made = 0usize;
    let mut skipped = 0usize;

    for line in text.lines() {
        let name = line.trim();
        if name.is_empty() {
            continue;
        }
        let normalized = name.replace('\\', "/");
        if normalized.split('/').any(|p| p == ".." || p.trim() == ".") {
            skipped += 1;
            continue;
        }
        // Windows 不允许的字符换成下划线，否则整条建不出来
        let safe: PathBuf = normalized
            .split('/')
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                p.chars()
                    .map(|c| {
                        if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect::<String>()
            })
            .collect();
        if safe.as_os_str().is_empty() {
            skipped += 1;
            continue;
        }
        let target = base.join(&safe);
        if long_path(&target).exists() {
            skipped += 1;
            continue;
        }
        match std::fs::create_dir_all(long_path(&target)) {
            Ok(_) => made += 1,
            Err(_) => skipped += 1,
        }
    }

    if made == 0 {
        return Err(AppError::new("err.noDirsMade"));
    }
    Ok((base, made, skipped))
}

#[tauri::command]
pub async fn dirs_create(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, |src| {
            let (dir, made, skipped) = make_dirs(src)?;
            let mut note = Note::new("note.dirsMade").with("n", made);
            if skipped > 0 {
                note = note.plus("note.dirsSkipped").with("n", skipped);
            }
            Ok((dir, Some(note)))
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
