use crate::err::AppResult;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 处理结果统一写入的目录名。安全红线 1：绝不覆盖原文件。
pub const OUTPUT_DIR: &str = "Baobox_output";

/// 用户指定的输出目录。为空则沿用「源文件旁边的 Baobox_output」。
///
/// 做成全局而不是逐层传参：十四处调用点都只拿得到源文件路径，
/// 为了一个设置项把签名全改一遍不划算，而这是单用户桌面程序，
/// 同一时刻只有一批任务在跑。
static OUTPUT_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

fn set_output_root(dir: Option<PathBuf>) {
    if let Ok(mut g) = OUTPUT_ROOT.lock() {
        *g = dir;
    }
}

/// 设置输出目录。传 None 恢复默认的「源文件旁边的 Baobox_output」。
///
/// 先验证目录存在再落地——写进一个不存在的地方，要等整批跑完才发现，
/// 那时候时间已经白花了。
#[tauri::command]
pub fn set_output_dir(dir: Option<String>) -> Result<(), String> {
    match dir {
        None => {
            set_output_root(None);
            Ok(())
        }
        Some(d) => {
            let pb = PathBuf::from(&d);
            if !long_path(&pb).is_dir() {
                return Err("err.outDirMissing".into());
            }
            set_output_root(Some(pb));
            Ok(())
        }
    }
}

fn output_root() -> Option<PathBuf> {
    OUTPUT_ROOT.lock().ok().and_then(|g| g.clone())
}

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

/// 为源文件准备输出目录。
///
/// 默认在源文件旁边建 `Baobox_output/`；用户指定过就直接用他指定的那个。
/// 注意两者的覆盖策略不同 —— 见 [`unique_path`]。
pub fn output_dir_for(src: &Path) -> AppResult<PathBuf> {
    let dir = match output_root() {
        Some(root) => root,
        None => src
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(OUTPUT_DIR),
    };
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
///
/// 用户自己指定的输出目录**不算**我们的地盘 —— 那里面可能本来就有他的东西，
/// 一律走加后缀的路子，宁可多留一份也不覆盖。
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{ext}"));
    // 只有它确实是我们自建的输出目录、且不是用户指定的，才允许覆盖
    let ours = output_root().is_none() && dir.file_name().map(|n| n == OUTPUT_DIR).unwrap_or(false);
    if ours || !long_path(&candidate).exists() {
        return candidate;
    }
    // 用户自选目录，或者理论上不该出现的情况——加后缀，绝不覆盖
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
