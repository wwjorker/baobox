use crate::err::{AppError, AppResult};
use crate::paths::long_path;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 批量重命名
///
/// 三条约束决定了实现方式：
///   · 规则可叠加——真实需求往往是「去掉前缀 + 统一小写 + 加序号」的组合
///   · 必须先预览——重命名一旦执行就散落在文件系统里，没有预览等于闭眼操作
///   · 必须能撤销——预览也会看漏，撤销日志是最后一道防线

#[derive(Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Rule {
    /// 正则替换
    Regex { find: String, replace: String },
    /// 普通文本替换
    Replace { find: String, replace: String },
    /// 加前缀
    Prefix { text: String },
    /// 加后缀（在扩展名之前）
    Suffix { text: String },
    /// 插入序号
    Number {
        start: u32,
        /// 位数，不足补零
        digits: u32,
        /// 序号放在前面还是后面
        prefix: bool,
    },
    /// 大小写
    Case { mode: String },
}

#[derive(Serialize, Clone)]
pub struct Preview {
    pub path: String,
    pub old_name: String,
    pub new_name: String,
    /// 与同批次里另一个结果重名，或目标已存在
    pub conflict: bool,
    /// 含 Windows 不允许的字符
    pub invalid: bool,
    pub unchanged: bool,
}

/// Windows 文件名里不能出现的字符
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

fn split_name(p: &Path) -> (String, String) {
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    (stem, ext)
}

fn apply_rules(stem: &str, rules: &[Rule], index: usize) -> String {
    let mut s = stem.to_string();
    for rule in rules {
        s = match rule {
            Rule::Regex { find, replace } => match regex::Regex::new(find) {
                Ok(re) => re.replace_all(&s, replace.as_str()).to_string(),
                // 正则写错时保持原样，而不是把文件名清空——
                // 用户还在敲的半截正则不该毁掉预览
                Err(_) => s,
            },
            Rule::Replace { find, replace } => {
                if find.is_empty() {
                    s
                } else {
                    s.replace(find, replace)
                }
            }
            Rule::Prefix { text } => format!("{text}{s}"),
            Rule::Suffix { text } => format!("{s}{text}"),
            Rule::Number { start, digits, prefix } => {
                let n = *start as usize + index;
                let num = format!("{:0width$}", n, width = *digits as usize);
                if *prefix {
                    format!("{num}{s}")
                } else {
                    format!("{s}{num}")
                }
            }
            Rule::Case { mode } => match mode.as_str() {
                "lower" => s.to_lowercase(),
                "upper" => s.to_uppercase(),
                "title" => s
                    .split(' ')
                    .map(|w| {
                        let mut c = w.chars();
                        match c.next() {
                            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                            None => String::new(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => s,
            },
        };
    }
    s
}

/// 生成预览。冲突和非法字符在这里就标出来，不等到执行才报错。
#[tauri::command]
pub fn rename_preview(paths: Vec<String>, rules: Vec<Rule>) -> Vec<Preview> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out: Vec<Preview> = Vec::with_capacity(paths.len());

    for (i, p) in paths.iter().enumerate() {
        let path = PathBuf::from(p);
        let (stem, ext) = split_name(&path);
        let new_stem = apply_rules(&stem, &rules, i);
        let new_name = format!("{new_stem}{ext}");

        let invalid = new_stem.is_empty() || new_name.chars().any(|c| FORBIDDEN.contains(&c));
        let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let key = dir.join(&new_name).to_string_lossy().to_lowercase();
        // 同一批里撞名，或者目标位置本来就有别的文件
        let dup_in_batch = *seen.get(&key).unwrap_or(&0) > 0;
        let exists_other = long_path(&dir.join(&new_name)).exists()
            && !key.eq(&path.to_string_lossy().to_lowercase());
        *seen.entry(key).or_insert(0) += 1;

        out.push(Preview {
            unchanged: new_name == format!("{stem}{ext}"),
            old_name: format!("{stem}{ext}"),
            new_name,
            conflict: dup_in_batch || exists_other,
            invalid,
            path: p.clone(),
        });
    }
    out
}

#[derive(Serialize, Clone)]
pub struct RenameResult {
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
    /// 撤销日志的存放位置，执行后交给前端记着
    pub undo_log: String,
}

#[derive(Serialize, Deserialize)]
struct UndoEntry {
    from: String,
    to: String,
}

/// 执行重命名，并写一份撤销日志。
///
/// 冲突和非法命名一律跳过而不是硬改——宁可少改几个，
/// 也不能覆盖掉一个同名文件。
///
/// **撤销日志必须先能落盘，才允许动第一个文件。** 之前是全部改完后
/// 一句 `let _ = write(...)` 把写盘失败吞了：磁盘满 / 无权限时会出现
/// 「已经改了名，却拿到一个并不存在的撤销日志」——安全承诺的洞。
/// 现在先建好日志文件验证可写（建不起来就直接报错、一个都不改），
/// 再每改一个就追加一行并 flush，中途崩溃也留下已完成部分的可撤销记录。
#[tauri::command]
pub fn rename_apply(paths: Vec<String>, rules: Vec<Rule>) -> AppResult<RenameResult> {
    let previews = rename_preview(paths, rules);

    // 撤销日志和文件放一起，用户即使换了会话也能找回来；
    // 源目录写不进去（只读盘、权限）就退回临时目录，两处都不行才放弃。
    let sidecar_dir = previews
        .first()
        .map(|p| PathBuf::from(&p.path))
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let (log_path, mut log_file) = open_undo_log(sidecar_dir.as_deref())?;

    let (mut done, mut skipped, mut failed) = (0usize, 0usize, 0usize);

    for pv in &previews {
        if pv.conflict || pv.invalid || pv.unchanged {
            skipped += 1;
            continue;
        }
        let from = PathBuf::from(&pv.path);
        let Some(dir) = from.parent() else {
            failed += 1;
            continue;
        };
        let to = dir.join(&pv.new_name);
        match std::fs::rename(long_path(&from), long_path(&to)) {
            Ok(_) => {
                done += 1;
                let entry = UndoEntry {
                    from: from.to_string_lossy().to_string(),
                    to: to.to_string_lossy().to_string(),
                };
                // 一行一条 JSON（JSONL）：追加即可，flush 落盘，崩溃也不丢已完成的
                if let Ok(line) = serde_json::to_string(&entry) {
                    let _ = writeln!(log_file, "{line}");
                    let _ = log_file.flush();
                }
            }
            Err(_) => failed += 1,
        }
    }
    drop(log_file);

    // 一个都没改就别在用户目录里留个空日志
    let undo_log = if done > 0 {
        log_path.to_string_lossy().to_string()
    } else {
        let _ = std::fs::remove_file(long_path(&log_path));
        String::new()
    };

    Ok(RenameResult {
        done,
        skipped,
        failed,
        undo_log,
    })
}

/// 建好撤销日志文件并验证可写。源目录优先，失败退临时目录，都不行才报错。
fn open_undo_log(sidecar_dir: Option<&Path>) -> AppResult<(PathBuf, std::fs::File)> {
    let name = format!("Baobox 重命名撤销 {}.jsonl", chrono_stamp());
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(d) = sidecar_dir {
        candidates.push(d.join(&name));
    }
    candidates.push(std::env::temp_dir().join(&name));

    for path in &candidates {
        if let Ok(f) = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(long_path(path))
        {
            return Ok((path.clone(), f));
        }
    }
    Err(AppError::new("err.undoLogFailed"))
}

/// 按撤销日志把名字改回去。日志是 JSONL（一行一条），逐行解析。
#[tauri::command]
pub fn rename_undo(log_path: String) -> AppResult<usize> {
    let data = std::fs::read_to_string(long_path(Path::new(&log_path)))?;
    let entries: Vec<UndoEntry> = data
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let mut restored = 0usize;
    // 倒序还原，避免链式重命名时中途撞名
    for e in entries.iter().rev() {
        if std::fs::rename(long_path(Path::new(&e.to)), long_path(Path::new(&e.from))).is_ok() {
            restored += 1;
        }
    }
    if restored > 0 {
        let _ = std::fs::remove_file(long_path(Path::new(&log_path)));
    }
    Ok(restored)
}

fn chrono_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
