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

/// 在输出目录里取文件名。
///
/// 「绝不覆盖」这条红线守的是**用户的原文件**，不是我们自己上一次的产物。
/// 早先一律加 (2)(3) 后缀，结果同一批文件跑三遍就堆出三份，输出目录很快
/// 变成垃圾场，而用户想要的几乎总是最新那份。
///
/// 所以现在：`Baobox_output` 里我们自己产出的同名文件直接覆盖；
/// 目录之外的任何东西一律不碰。真要留旧结果，重命名或移走即可。
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{ext}"));
    // 只有当它确实位于我们的输出目录里时才允许覆盖
    let ours = dir.file_name().map(|n| n == OUTPUT_DIR).unwrap_or(false);
    if ours || !long_path(&candidate).exists() {
        return candidate;
    }
    // 不在输出目录里（理论上不该发生）——退回加后缀，宁可多一份也不覆盖
    let mut n = 2;
    loop {
        let alt = dir.join(format!("{stem} ({n}).{ext}"));
        if !long_path(&alt).exists() {
            return alt;
        }
        n += 1;
    }
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
