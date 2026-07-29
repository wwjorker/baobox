//! 文件粉碎：多次覆写后永久删除，不进回收站。
//!
//! # 这是整个软件里唯一会不可逆销毁数据的功能
//!
//! 安全红线明确要求它与普通删除**彻底隔离**：独立入口、独立的红色确认流程、
//! 要求手动输入确认短语、明确告知不可恢复，绝不与「删除进回收站」共用任何
//! 代码路径。这个文件就是那条独立路径，它**不导出**任何被普通删除复用的东西。
//!
//! # 后端自己也要设防，不能只靠界面
//!
//! 命令要求传入一个跟内置短语完全一致的 `confirm` 字符串。这样即使界面某个
//! 按钮接错了线，少了这一步也粉碎不了任何东西——把「别误删」这条保障做进
//! 后端，而不是全押在前端不出 bug 上。
//!
//! # 诚实交代 SSD 的局限
//!
//! 覆写在机械硬盘上能可靠销毁数据，但 SSD 有磨损均衡，覆写落到的往往是
//! 另一块物理区域，原数据可能仍残留。这一点界面上如实说明，不假装覆写
//! 在哪儿都等于抹除——安全功能尤其不能给人虚假的安全感。

use crate::err::{AppError, AppResult};
use crate::paths::{file_name_of, long_path};
use serde::Serialize;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

/// 必须一字不差传进来的确认短语。改动它等于改动一份用户已经习惯的契约，
/// 不要轻易动。
pub const CONFIRM_PHRASE: &str = "粉碎";

#[derive(Serialize, Clone)]
pub struct ShredOutcome {
    pub path: String,
    pub name: String,
    pub ok: bool,
    pub error: Option<AppError>,
}

#[derive(Serialize, Clone)]
struct ShredProgress {
    index: usize,
    total: usize,
    outcome: ShredOutcome,
}

/// 覆写一个文件若干遍，再永久删除。
///
/// 覆写模式故意混用：先全 0、再全 1、最后随机，比单一模式更难从残磁恢复。
/// 每遍都 flush + sync，确保真的落盘而不是停在系统缓存里。
fn shred_one(path: &Path, passes: u32) -> AppResult<()> {
    let meta = std::fs::metadata(long_path(path))?;
    if meta.is_dir() {
        return Err(AppError::new("err.shredNoDir"));
    }
    let len = meta.len();

    // 只读文件直接 open(write) 会失败，先把只读属性去掉
    let mut perms = meta.permissions();
    if perms.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(long_path(path), perms);
    }

    if len > 0 {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(long_path(path))?;
        let mut buf = vec![0u8; (len.min(1 << 20)) as usize];

        for pass in 0..passes.max(1) {
            f.seek(SeekFrom::Start(0))?;
            let mut written = 0u64;
            while written < len {
                let chunk = ((len - written) as usize).min(buf.len());
                match pass % 3 {
                    0 => buf[..chunk].fill(0x00),
                    1 => buf[..chunk].fill(0xFF),
                    _ => fill_pseudo_random(&mut buf[..chunk], written ^ pass as u64),
                }
                f.write_all(&buf[..chunk])?;
                written += chunk as u64;
            }
            f.flush()?;
            // 强制落盘：不 sync 的话覆写可能只到了缓存，掉电前原数据还在
            f.sync_all()?;
        }
    }

    // 连文件名一起抹掉痕迹：先改成等长的随机名再删，
    // 免得文件名本身（可能含敏感信息）留在目录项里
    let renamed = rename_to_random(path)?;
    std::fs::remove_file(long_path(&renamed))?;
    Ok(())
}

/// 供验收测试直接调用，不经过命令层和确认短语。
pub fn shred_one_for_test(path: &Path, passes: u32) -> AppResult<()> {
    shred_one(path, passes)
}

/// 一个够用的伪随机填充，不引入 rng 依赖。种子混入偏移量，各遍不重样。
fn fill_pseudo_random(buf: &mut [u8], seed: u64) {
    let mut state = seed
        ^ 0x9E37_79B9_7F4A_7C15
        ^ std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
    for b in buf.iter_mut() {
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *b = (state >> 24) as u8;
    }
}

fn rename_to_random(path: &Path) -> AppResult<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..10 {
        let mut name = String::new();
        let mut s = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        for _ in 0..16 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            name.push((b'a' + (s % 26) as u8) as char);
        }
        let candidate = parent.join(name);
        if !long_path(&candidate).exists() {
            std::fs::rename(long_path(path), long_path(&candidate))?;
            return Ok(candidate);
        }
    }
    // 实在撞名就用原路径删，覆写已经完成，痕迹清理是加分项不是必要项
    Ok(path.to_path_buf())
}

/// 粉碎一批文件。
///
/// `confirm` 必须等于 [`CONFIRM_PHRASE`]，否则整批拒绝——这是后端的自保，
/// 不依赖界面。见文件头说明。
#[tauri::command]
pub async fn shred_files(
    app: AppHandle,
    paths: Vec<String>,
    passes: u32,
    confirm: String,
) -> Result<Vec<ShredOutcome>, String> {
    if confirm != CONFIRM_PHRASE {
        // 这不该发生（界面会挡），发生了就是接线错误，明确拒绝
        return Err("err.shredNotConfirmed".into());
    }
    let n = passes.clamp(1, 7);

    Ok(tauri::async_runtime::spawn_blocking(move || {
        let total = paths.len();
        let mut out = Vec::with_capacity(total);
        for (index, p) in paths.iter().enumerate() {
            let src = PathBuf::from(p);
            let outcome = match shred_one(&src, n) {
                Ok(()) => ShredOutcome {
                    path: p.clone(),
                    name: file_name_of(&src),
                    ok: true,
                    error: None,
                },
                Err(e) => ShredOutcome {
                    path: p.clone(),
                    name: file_name_of(&src),
                    ok: false,
                    error: Some(e),
                },
            };
            let _ = app.emit(
                "baobox://shred",
                ShredProgress {
                    index,
                    total,
                    outcome: outcome.clone(),
                },
            );
            out.push(outcome);
        }
        out
    })
    .await
    .unwrap_or_default())
}
