use crate::err::AppResult;
use std::path::{Path, PathBuf};

/// 处理结果统一写入的目录名。安全红线 1：绝不覆盖原文件。
pub const OUTPUT_DIR: &str = "Baobox_output";

/// 给路径加上 Windows 的 `\\?\` 前缀，绕开 260 字符上限。
///
/// 方案风险 16：中文路径 + 深层目录 + 长文件名极易撞上这个限制，
/// 而报错信息完全不指向路径问题，用户只会看到莫名其妙的「文件不存在」。
#[cfg(windows)]
pub fn long_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    // UNC 路径和已加前缀的不重复处理
    if s.starts_with(r"\\?\") || s.starts_with(r"\\") {
        return p.to_path_buf();
    }
    // 只有绝对路径能用这个前缀
    if p.is_absolute() {
        PathBuf::from(format!(r"\\?\{s}"))
    } else {
        p.to_path_buf()
    }
}

#[cfg(not(windows))]
pub fn long_path(p: &Path) -> PathBuf {
    p.to_path_buf()
}

/// 为源文件准备输出目录：在它旁边建 `Baobox_output/`
pub fn output_dir_for(src: &Path) -> AppResult<PathBuf> {
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    let dir = parent.join(OUTPUT_DIR);
    std::fs::create_dir_all(long_path(&dir))?;
    Ok(dir)
}

/// 在输出目录里取一个不冲突的文件名。
///
/// 即使是输出目录里的已有文件也不覆盖——用户可能跑了两次，
/// 第二次的结果不该悄悄吃掉第一次的。
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while long_path(&candidate).exists() {
        candidate = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    candidate
}

pub fn stem_of(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "output".into())
}

pub fn file_name_of(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}
