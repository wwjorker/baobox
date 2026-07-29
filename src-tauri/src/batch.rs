use crate::err::{AppError, AppResult};
use crate::paths::{file_name_of, long_path};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// 取消标志。
///
/// 检查点放在「文件与文件之间」而不是编码循环内部：一次编码通常几百毫秒，
/// 等它跑完再停，响应已经够快；而在编码中途硬停会留下半截文件。
/// 已经处理完的产物一律保留（方案风险 18）。
static CANCEL: AtomicBool = AtomicBool::new(false);

/// 中止当前批次。已经处理完的产物保留，没轮到的标记为「已取消」。
#[tauri::command]
pub fn cancel_batch() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// 每批任务开工前调一次，清掉上一批可能残留的标志
pub fn reset_cancel() {
    CANCEL.store(false, Ordering::SeqCst);
}

pub fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

/// 结果行右边那句附加说明，如「12 页 → 3 份」。
///
/// 带 key 和占位符，由界面按当前语言渲染 —— 早先这里直接塞中文字符串，
/// 英文界面下整整一列都是中文。i18n 层从第一行代码就搭好了
/// （方案风险 15），却在最后一米漏成这样。
#[derive(Serialize, Clone)]
pub struct NotePart {
    pub key: String,
    pub vars: BTreeMap<String, String>,
}

/// 由若干片段组成，界面用「 · 」接起来。
///
/// 分成片段是因为像「质量 78 · 缩放 62% · 未能达标」这种说明是按情况拼的，
/// 拼好的整句没法翻译，拆成片段每段各自查表就行。
#[derive(Serialize, Clone)]
pub struct Note {
    pub parts: Vec<NotePart>,
}

impl Note {
    pub fn new(key: &str) -> Self {
        Self {
            parts: vec![NotePart {
                key: key.to_string(),
                vars: BTreeMap::new(),
            }],
        }
    }

    /// 给最后一个片段加占位符
    pub fn with(mut self, name: &str, value: impl std::fmt::Display) -> Self {
        if let Some(last) = self.parts.last_mut() {
            last.vars.insert(name.to_string(), value.to_string());
        }
        self
    }

    /// 追加一个片段
    pub fn plus(mut self, key: &str) -> Self {
        self.parts.push(NotePart {
            key: key.to_string(),
            vars: BTreeMap::new(),
        });
        self
    }
}

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
    pub note: Option<Note>,
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
    pub fn ok(src: &Path, dst: PathBuf, note: Option<Note>) -> Self {
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

    /// 没轮到就被取消了。不算失败——用户自己喊停的，不该在界面上显示成红色的错。
    pub fn skipped(src: &Path, reason_key: &str) -> Self {
        let mut o = Self::fail(src, AppError::new(reason_key));
        o.error = None;
        o.note = Some(Note::new(reason_key));
        o
    }

    /// 已被并进另一份产物里。
    ///
    /// 合并、图片转 PDF 这类 N→1 的操作，产物只有一份，挂在第一个输入上。
    /// 其余输入原先什么结果都不发，界面上就永远停在「等待」——看着像卡死了。
    pub fn folded(src: &Path) -> Self {
        let in_bytes = std::fs::metadata(long_path(src)).map(|m| m.len()).unwrap_or(0);
        Self {
            path: src.to_string_lossy().to_string(),
            name: file_name_of(src),
            ok: true,
            in_bytes,
            // 产物不属于这一条，报 0 会被界面算成「省下了全部体积」
            out_bytes: in_bytes,
            out_path: None,
            note: Some(Note::new("run.foldedInto")),
            text: None,
            error: None,
        }
    }
}

/// N→1 的工具（合并、图片转 PDF）给每个输入各配一条结果。
///
/// 产物只有一份，挂在第一个输入上，`rest` 是其余输入。抽成纯函数是为了能测：
/// 「每个输入都得有一条结果」正是之前漏掉的那条——只发第一条，
/// 界面上后面几行就永远停在「等待」，看着像处理到一半卡死了。
pub fn fold_outcomes(head: FileOutcome, rest: &[PathBuf]) -> Vec<FileOutcome> {
    let produced = head.ok;
    let mut out = Vec::with_capacity(rest.len() + 1);
    out.push(head);
    for src in rest {
        out.push(if produced {
            FileOutcome::folded(src)
        } else {
            FileOutcome::skipped(src, "run.mergeFailedSkip")
        });
    }
    out
}

/// 逐个文件处理并即时上报。
///
/// 结果逐条提交而不是最后统一写入：中途取消或崩溃时，
/// 已完成的部分依然留在磁盘上，不会白干（方案风险 18）。
pub fn run_batch<F>(app: &AppHandle, paths: Vec<String>, job: F) -> Vec<FileOutcome>
where
    F: Fn(&Path) -> AppResult<(PathBuf, Option<Note>)>,
{
    let total = paths.len();
    let mut out = Vec::with_capacity(total);
    reset_cancel();

    for (index, p) in paths.iter().enumerate() {
        let src = PathBuf::from(p);

        // 取消后剩下的照样要发结果出去，否则那些行会一直转着「等待」，
        // 用户分不清是停了还是卡了
        if cancelled() {
            let o = FileOutcome::skipped(&src, "run.cancelledSkip");
            emit(app, index, total, &o);
            out.push(o);
            continue;
        }

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
