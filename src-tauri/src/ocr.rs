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

/// 一个识别出来的词，带它在图上的位置。
///
/// 「扫描件转可搜索 PDF」靠的就是这些坐标——把文字按原位盖成不可见的一层，
/// 搜索和选取才落在图上对的地方。只拿纯文本是做不出来的。
#[derive(Clone, Debug)]
pub struct OcrWord {
    pub text: String,
    /// 单位是输入图的像素，左上角为原点
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct OcrPage {
    pub words: Vec<OcrWord>,
    /// 输入图的尺寸，用来把像素坐标换算成 PDF 的点
    pub img_w: u32,
    pub img_h: u32,
}

/// 识别并保留每个词的位置。
pub fn recognize_words(path: &Path, lang: Option<&str>) -> AppResult<OcrPage> {
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

    let img_w = bitmap.PixelWidth().unwrap_or(0).max(0) as u32;
    let img_h = bitmap.PixelHeight().unwrap_or(0).max(0) as u32;

    let engine = engine_for(lang)?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(|op| op.get())
        .map_err(|e| AppError::unknown(e))?;

    let mut words = Vec::new();
    if let Ok(ls) = result.Lines() {
        for line in ls {
            let Ok(ws) = line.Words() else { continue };
            // 中文按字给词，逐字盖一层文字会让选取碎成一个个字。
            // 先按同一行、间距不大的原则并成连续片段再盖。
            let mut run: Vec<(String, f32, f32, f32, f32)> = Vec::new();
            for w in ws {
                let (Ok(t), Ok(r)) = (w.Text(), w.BoundingRect()) else {
                    continue;
                };
                let t = t.to_string();
                if t.is_empty() {
                    continue;
                }
                run.push((t, r.X, r.Y, r.Width, r.Height));
            }
            words.extend(merge_run(run));
        }
    }

    Ok(OcrPage {
        words,
        img_w,
        img_h,
    })
}

/// 把一行里挨得很近的词并成一段。
///
/// WinRT 把每个汉字当一个词，不合并的话「百宝箱」会盖成三段互不相连的文字，
/// 在阅读器里拖选是一个字一个字跳的，跨词搜索也搜不到。
/// 间距超过一个字宽就认为是真的分开了，保持原样。
fn merge_run(items: Vec<(String, f32, f32, f32, f32)>) -> Vec<OcrWord> {
    let mut out: Vec<OcrWord> = Vec::new();
    for (text, x, y, w, h) in items {
        if let Some(prev) = out.last_mut() {
            let gap = x - (prev.x + prev.w);
            let same_line = (y - prev.y).abs() < prev.h * 0.5;
            if same_line && gap < prev.h * 0.6 {
                // 拉丁词之间要留空格，中日韩之间不留（同 join_words 的规则）
                let need_space = !is_cjk_edge(&prev.text, &text) && gap > prev.h * 0.15;
                if need_space {
                    prev.text.push(' ');
                }
                prev.text.push_str(&text);
                prev.w = x + w - prev.x;
                prev.h = prev.h.max(h);
                continue;
            }
        }
        out.push(OcrWord { text, x, y, w, h });
    }
    out
}

/// 两段文字的接缝处是不是中日韩字符——是的话不该插空格
fn is_cjk_edge(left: &str, right: &str) -> bool {
    let a = left.chars().last();
    let b = right.chars().next();
    matches!((a, b), (Some(a), Some(b)) if is_cjk(a) || is_cjk(b))
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
