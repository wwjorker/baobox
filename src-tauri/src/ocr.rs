use crate::err::{AppError, AppResult};
use crate::paths::{file_name_of, long_path, output_dir_for, stem_of, unique_path};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use windows::core::HSTRING;
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};

/// 判断是否为 CJK 字符（含标点与全角形式）
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2EFF
        | 0x3000..=0x303F
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFFEF
    )
}

/// 按词拼接一行，只在两侧都不是 CJK 时才插空格。
///
/// WinRT 的 OCR 引擎把每个汉字当作独立的「词」，直接用它的 Text()
/// 会得到「百 宝 箱 本 地 文 件」这种结果——中文用户复制出去没法用，
/// 功能等于废掉。在拼接阶段处理比事后剥离空格更准确，因为这里能
/// 确切知道词的边界，不用去猜哪个空格是引擎加的、哪个是原文里的。
fn join_words(words: &[String]) -> String {
    let mut out = String::new();
    for w in words {
        if out.is_empty() {
            out.push_str(w);
            continue;
        }
        let prev = out.chars().last().unwrap_or(' ');
        let next = w.chars().next().unwrap_or(' ');
        if !(is_cjk(prev) && is_cjk(next)) {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// 系统装了哪些 OCR 语言。缺中文时要给出明确的安装引导，
/// 而不是识别出一堆乱码后让用户自己纳闷（方案风险 11）。
#[tauri::command]
pub fn ocr_languages() -> Vec<String> {
    ensure_com();
    let Ok(langs) = OcrEngine::AvailableRecognizerLanguages() else {
        return Vec::new();
    };
    langs
        .into_iter()
        .filter_map(|l| l.LanguageTag().ok().map(|t| t.to_string()))
        .collect()
}

/// 在当前线程上完成 COM 初始化。
///
/// Tauri 的工作线程默认没有初始化 COM，而 WinRT 的
/// `IAsyncOperation::get()` 在未初始化的线程上会直接挂死——
/// 实测就是这么卡住的，界面停在「处理中…」再无反应。
/// 用 MTA 模式，因为我们本来就是在后台线程阻塞等待结果。
fn ensure_com() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    // 已经初始化过会返回 RPC_E_CHANGED_MODE / S_FALSE，都无需处理
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// 按语言标签取引擎；给不出就退回用户配置的默认语言。
///
/// 引擎和语言必须配对——用中文引擎去认英文，会把 Invoice 认成
/// 「| nvo i ce」、把连字符认成「一」。这不是后处理能修的，
/// 只能选对引擎。
fn engine_for(lang: Option<&str>) -> AppResult<OcrEngine> {
    if let Some(tag) = lang.filter(|t| !t.is_empty()) {
        if let Ok(l) = windows::Globalization::Language::CreateLanguage(&HSTRING::from(tag)) {
            if let Ok(e) = OcrEngine::TryCreateFromLanguage(&l) {
                return Ok(e);
            }
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| AppError::new("err.ocrNoLanguage").detail(e))
}

fn recognize(path: &Path, lang: Option<&str>) -> AppResult<String> {
    ensure_com();
    let p = path.to_string_lossy().to_string();
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(p.as_str()))
        .and_then(|op| op.get())
        .map_err(|e| AppError::new("err.notFound").detail(e))?;

    let stream = file
        .OpenAsync(FileAccessMode::Read)
        .and_then(|op| op.get())
        .map_err(|e| AppError::unknown(e))?;

    let decoder = BitmapDecoder::CreateAsync(&stream)
        .and_then(|op| op.get())
        .map_err(|e| AppError::decode("图片", e))?;

    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .and_then(|op| op.get())
        .map_err(|e| AppError::decode("图片", e))?;

    let engine = engine_for(lang)?;

    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(|op| op.get())
        .map_err(|e| AppError::unknown(e))?;

    let mut lines = Vec::new();
    if let Ok(ls) = result.Lines() {
        for line in ls {
            let mut words = Vec::new();
            if let Ok(ws) = line.Words() {
                for w in ws {
                    if let Ok(t) = w.Text() {
                        words.push(t.to_string());
                    }
                }
            }
            if !words.is_empty() {
                lines.push(join_words(&words));
            }
        }
    }
    Ok(lines.join("\n"))
}

/// 供验收测试直接调用的识别入口，不经过批量层和 AppHandle
pub fn recognize_for_test(path: &Path) -> AppResult<String> {
    recognize(path, None)
}

/// 给截图取字用：识别单张图，可指定语言
pub fn recognize_with_lang(path: &Path, lang: Option<&str>) -> AppResult<String> {
    recognize(path, lang)
}

#[derive(Serialize, Clone)]
pub struct OcrOutcome {
    pub path: String,
    pub name: String,
    pub ok: bool,
    pub in_bytes: u64,
    pub out_bytes: u64,
    pub out_path: Option<String>,
    pub note: Option<crate::batch::Note>,
    /// 没轮到就被取消了。见 FileOutcome::skipped——不是失败，别画成红叉。
    #[serde(default)]
    pub skipped: bool,
    /// 识别出的文字。OCR 的产物是文本而不是文件，界面要能直接看和复制。
    pub text: Option<String>,
    pub error: Option<AppError>,
}

#[derive(Serialize, Clone)]
struct OcrProgress {
    index: usize,
    total: usize,
    outcome: OcrOutcome,
}

/// 批量识别并合并成一份文本，每段带文件名标题。
///
/// 和 `ocr_image` 的区别是产物形态：这个给的是一份可直接归档的整合文档，
/// 适合「把一叠扫描件转成一份可搜索的文字稿」这类场景。
#[tauri::command]
pub async fn ocr_batch(app: AppHandle, paths: Vec<String>, lang: Option<String>) -> Vec<OcrOutcome> {
    tauri::async_runtime::spawn_blocking(move || ocr_batch_blocking(app, paths, lang))
        .await
        .unwrap_or_default()
}

fn ocr_batch_blocking(app: AppHandle, paths: Vec<String>, lang: Option<String>) -> Vec<OcrOutcome> {
    let results = ocr_blocking(app, paths, lang);

    let merged: String = results
        .iter()
        .filter(|r| r.ok)
        .map(|r| {
            format!(
                "===== {} =====\n{}\n",
                r.name,
                r.text.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    if let Some(first) = results.iter().find(|r| r.ok) {
        let src = PathBuf::from(&first.path);
        if let Ok(dir) = output_dir_for(&src) {
            let dst = unique_path(&dir, "OCR 合并结果", "txt");
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(merged.as_bytes());
            let _ = std::fs::write(long_path(&dst), &bytes);
        }
    }

    results
}

/// 异步入口：把阻塞工作丢到线程池，否则会冻住整个界面。
/// Tauri 的同步命令跑在主线程上，长任务会让窗口失去响应、
/// 进度事件也渲染不出来。
#[tauri::command]
pub async fn ocr_image(app: AppHandle, paths: Vec<String>, lang: Option<String>) -> Vec<OcrOutcome> {
    tauri::async_runtime::spawn_blocking(move || ocr_blocking(app, paths, lang))
        .await
        .unwrap_or_default()
}

fn ocr_blocking(app: AppHandle, paths: Vec<String>, lang: Option<String>) -> Vec<OcrOutcome> {
    let total = paths.len();
    let mut results = Vec::with_capacity(total);
    // OCR 没走 run_batch（产物是文本不是文件），取消检查得自己做一遍
    crate::batch::reset_cancel();

    for (index, p) in paths.iter().enumerate() {
        let src = PathBuf::from(p);
        let in_bytes = std::fs::metadata(long_path(&src)).map(|m| m.len()).unwrap_or(0);

        if crate::batch::cancelled() {
            let o = OcrOutcome {
                path: p.clone(),
                name: file_name_of(&src),
                ok: false,
                in_bytes,
                out_bytes: 0,
                out_path: None,
                note: Some(crate::batch::Note::new("run.cancelledSkip")),
                skipped: true,
                text: None,
                error: None,
            };
            let _ = app.emit(
                "baobox://progress",
                OcrProgress {
                    index,
                    total,
                    outcome: o.clone(),
                },
            );
            results.push(o);
            continue;
        }

        let outcome = match recognize(&src, lang.as_deref()) {
            Ok(text) => {
                // 同时落一份 .txt，方便批量场景下直接拿文件
                let written = (|| -> AppResult<PathBuf> {
                    let dir = output_dir_for(&src)?;
                    let dst = unique_path(&dir, &stem_of(&src), "txt");
                    // 带 BOM，免得用记事本打开中文变乱码
                    let mut bytes = vec![0xEF, 0xBB, 0xBF];
                    bytes.extend_from_slice(text.as_bytes());
                    std::fs::write(long_path(&dst), &bytes)?;
                    Ok(dst)
                })();

                let chars = text.chars().count();
                OcrOutcome {
                    path: p.clone(),
                    name: file_name_of(&src),
                    ok: true,
                    in_bytes,
                    // OCR 不是压缩：产物是文本，原图还在。早先这里报文本字节数，
                    // 界面拿 in-out 一减，就把「31 KB 的图识别出 200 字节文字」
                    // 算成省下 30 KB 记进了首页那个累计数字。
                    out_bytes: in_bytes,
                    out_path: written.as_ref().ok().map(|d| d.to_string_lossy().to_string()),
                    note: Some(if chars == 0 {
                        crate::batch::Note::new("note.ocrNone")
                    } else {
                        crate::batch::Note::new("note.ocrChars").with("chars", chars)
                    }),
                    skipped: false,
                    text: Some(text),
                    error: None,
                }
            }
            Err(e) => OcrOutcome {
                path: p.clone(),
                name: file_name_of(&src),
                ok: false,
                in_bytes,
                out_bytes: 0,
                out_path: None,
                note: None,
                skipped: false,
                text: None,
                error: Some(e),
            },
        };

        let _ = app.emit(
            "baobox://progress",
            OcrProgress {
                index,
                total,
                outcome: outcome.clone(),
            },
        );
        results.push(outcome);
    }
    results
}
