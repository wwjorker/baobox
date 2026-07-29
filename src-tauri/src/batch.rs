use crate::err::{AppError, AppResult};
use crate::paths::{file_name_of, long_path};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

/// 一个文件的处理结果。所有支柱共用同一套结构，
/// 前端的结果列表因此不需要为每类工具各写一遍。
#[derive(Serialize, Clone)]
pub struct FileOutcome {
    pub path: String,
    pub name: String,
    pub ok: bool,
    pub in_bytes: u64,
    pub out_bytes: u64,
    pub out_path: Option<String>,
    /// 附加说明，如「质量 78 · 缩放 85%」「12 页 → 3 份」
    pub note: Option<String>,
    /// 仅文本类工具会有
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub error: Option<AppError>,
}

#[derive(Serialize, Clone)]
pub struct Progress {
    pub index: usize,
    pub total: usize,
    pub outcome: FileOutcome,
}

impl FileOutcome {
    pub fn ok(src: &Path, dst: PathBuf, note: Option<String>) -> Self {
        let in_bytes = std::fs::metadata(long_path(src)).map(|m| m.len()).unwrap_or(0);
        let out_bytes = std::fs::metadata(long_path(&dst)).map(|m| m.len()).unwrap_or(0);
        Self {
            path: src.to_string_lossy().to_string(),
            name: file_name_of(src),
            ok: true,
            in_bytes,
            out_bytes,
            out_path: Some(dst.to_string_lossy().to_string()),
            note,
            text: None,
            error: None,
        }
    }

    pub fn fail(src: &Path, e: AppError) -> Self {
        let in_bytes = std::fs::metadata(long_path(src)).map(|m| m.len()).unwrap_or(0);
        Self {
            path: src.to_string_lossy().to_string(),
            name: file_name_of(src),
            ok: false,
            in_bytes,
            out_bytes: 0,
            out_path: None,
            note: None,
            text: None,
            error: Some(e),
        }
    }
}

/// 逐个文件处理并即时上报。
///
/// 结果逐条提交而不是最后统一写入：中途取消或崩溃时，
/// 已完成的部分依然留在磁盘上，不会白干（方案风险 18）。
pub fn run_batch<F>(app: &AppHandle, paths: Vec<String>, job: F) -> Vec<FileOutcome>
where
    F: Fn(&Path) -> AppResult<(PathBuf, Option<String>)>,
{
    let total = paths.len();
    let mut out = Vec::with_capacity(total);

    for (index, p) in paths.iter().enumerate() {
        let src = PathBuf::from(p);
        // 开工前先报一声。实测语料里有一份 PDF 解析要 254 秒，
        // 若只在完成后上报，用户会盯着毫无动静的界面以为程序死了。
        emit_working(app, index, total, &file_name_of(&src));

        let outcome = match job(&src) {
            Ok((dst, note)) => FileOutcome::ok(&src, dst, note),
            Err(e) => FileOutcome::fail(&src, e),
        };
        emit(app, index, total, &outcome);
        out.push(outcome);
    }
    out
}

#[derive(Serialize, Clone)]
struct Working {
    index: usize,
    total: usize,
    name: String,
}

pub fn emit_working(app: &AppHandle, index: usize, total: usize, name: &str) {
    let _ = app.emit(
        "baobox://working",
        Working {
            index,
            total,
            name: name.to_string(),
        },
    );
}

pub fn emit(app: &AppHandle, index: usize, total: usize, outcome: &FileOutcome) {
    let _ = app.emit(
        "baobox://progress",
        Progress {
            index,
            total,
            outcome: outcome.clone(),
        },
    );
}
