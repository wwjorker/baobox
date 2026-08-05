use crate::batch::{run_batch, FileOutcome, Note};
use crate::err::{AppError, AppResult};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use std::path::Path;
use tauri::AppHandle;
use windows::core::HSTRING;
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
use windows::Storage::StorageFile;

/// PDF 渲染成图片，用系统自带的 `Windows.Data.Pdf`。
///
/// 原方案打算引入 pdfium，但那要随包分发约 11 MB 的动态库。
/// Windows 自己就有一套 PDF 渲染引擎（Edge 用的同一套），
/// 调它等于零体积拿到同样的能力，安装包因此保持在 6 MB 量级。

fn ensure_com() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// 渲染指定页为 PNG 字节。`width` 是目标像素宽度，高度按页面比例自动算。
pub fn render_page(src: &Path, page_index: u32, width: u32) -> AppResult<Vec<u8>> {
    ensure_com();
    let p = src.to_string_lossy().to_string();
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(p.as_str()))
        .and_then(|o| o.get())
        .map_err(|e| AppError::new("err.notFound").detail(e))?;

    let doc = PdfDocument::LoadFromFileAsync(&file)
        .and_then(|o| o.get())
        // 加密的 PDF 系统引擎也打不开，指向解锁工具比抛原始错误有用
        .map_err(|e| {
            if e.to_string().to_lowercase().contains("password") {
                AppError::new("err.encrypted")
            } else {
                AppError::decode("PDF", e)
            }
        })?;

    let count = doc.PageCount().map_err(|e| AppError::unknown(e))?;
    if page_index >= count {
        return Err(AppError::new("err.pdfNoPages"));
    }

    let page = doc.GetPage(page_index).map_err(|e| AppError::unknown(e))?;
    let stream = InMemoryRandomAccessStream::new().map_err(|e| AppError::unknown(e))?;
    let opts = PdfPageRenderOptions::new().map_err(|e| AppError::unknown(e))?;
    opts.SetDestinationWidth(width).map_err(|e| AppError::unknown(e))?;
    page.RenderWithOptionsToStreamAsync(&stream, &opts)
        .and_then(|o| o.get())
        .map_err(|e| AppError::unknown(e))?;

    let size = stream.Size().map_err(|e| AppError::unknown(e))? as u32;
    let input = stream
        .GetInputStreamAt(0)
        .map_err(|e| AppError::unknown(e))?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| AppError::unknown(e))?;
    reader
        .LoadAsync(size)
        .and_then(|o| o.get())
        .map_err(|e| AppError::unknown(e))?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).map_err(|e| AppError::unknown(e))?;
    Ok(buf)
}

pub fn page_count(src: &Path) -> AppResult<u32> {
    ensure_com();
    let p = src.to_string_lossy().to_string();
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(p.as_str()))
        .and_then(|o| o.get())
        .map_err(|e| AppError::new("err.notFound").detail(e))?;
    let doc = PdfDocument::LoadFromFileAsync(&file)
        .and_then(|o| o.get())
        .map_err(|e| AppError::decode("PDF", e))?;
    doc.PageCount().map_err(|e| AppError::unknown(e))
}

/// DPI 换算成渲染宽度。PDF 的用户单位是 1/72 英寸，
/// 所以 150 DPI 意味着每个单位画 150/72 个像素。
fn width_for_dpi(dpi: u32) -> u32 {
    // 按 A4 宽度 595 单位估算，够用且避免为了拿尺寸多解析一次
    ((595.0 * dpi as f32 / 72.0) as u32).clamp(200, 10000)
}

/// 系统引擎渲染出来的一律是 PNG。要 JPG 就解码后重编码一遍。
/// 照片型扫描件转 JPG 能小很多，界面既然给了选项，后端就得真做到。
fn encode_page(png: Vec<u8>, jpg: bool) -> AppResult<(&'static str, Vec<u8>)> {
    if !jpg {
        return Ok(("png", png));
    }
    let rgb = image::load_from_memory(&png)
        .map_err(|e| AppError::decode("PNG", e))?
        .to_rgb8();
    let mut buf: Vec<u8> = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 88)
        .encode(&rgb, rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| AppError::unknown(e))?;
    Ok(("jpg", buf))
}

fn pdf_to_image_blocking(
    app: AppHandle,
    paths: Vec<String>,
    dpi: u32,
    format: String,
) -> Vec<FileOutcome> {
    let width = width_for_dpi(dpi);
    let jpg = format.eq_ignore_ascii_case("jpg") || format.eq_ignore_ascii_case("jpeg");
    let fmt_label = if jpg { "JPG" } else { "PNG" };
    run_batch(&app, paths, move |src| {
        let total = page_count(src)?;
        if total == 0 {
            return Err(AppError::new("err.pdfNoPages"));
        }
        let dir = output_dir_for(src)?;
        let stem = stem_of(src);
        let mut last = dir.clone();
        for i in 0..total {
            let png = render_page(src, i, width)?;
            let (ext, bytes) = encode_page(png, jpg)?;
            let dst = unique_path(&dir, &format!("{stem} 第{}页", i + 1), ext);
            std::fs::write(long_path(&dst), &bytes)?;
            last = dst;
        }
        Ok((
            last,
            Some(
                Note::new("note.pdfToImage")
                    .with("total", total)
                    .with("dpi", dpi)
                    .with("fmt", fmt_label),
            ),
        ))
    })
}

#[tauri::command]
pub async fn pdf_to_image(
    app: AppHandle,
    paths: Vec<String>,
    dpi: u32,
    format: String,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || pdf_to_image_blocking(app, paths, dpi, format))
        .await
        .unwrap_or_default()
}

// ============================================ 可视化整理页面用的缩略图

#[derive(serde::Serialize)]
pub struct PageThumbs {
    pub count: u32,
    /// 每页一张小图，data URL（PNG）。顺序即原文档页序。
    pub thumbs: Vec<String>,
}

/// 缩略图的目标宽度。小一点，为的是一次能把整份文档的每页都渲染出来。
const THUMB_WIDTH: u32 = 150;
/// 可视化整理最多支持的页数。再多就该用「拆分」，而且逐页渲染+传输会很沉。
const MAX_ORGANIZER_PAGES: u32 = 400;

fn page_thumbs_blocking(path: String) -> AppResult<PageThumbs> {
    use base64::Engine;
    let src = Path::new(&path);
    let total = page_count(src)?;
    if total == 0 {
        return Err(AppError::new("err.pdfNoPages"));
    }
    if total > MAX_ORGANIZER_PAGES {
        return Err(AppError::new("err.pdfTooManyPages").var("max", MAX_ORGANIZER_PAGES.to_string()));
    }
    let mut thumbs = Vec::with_capacity(total as usize);
    for i in 0..total {
        let png = render_page(src, i, THUMB_WIDTH)?;
        thumbs.push(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        ));
    }
    Ok(PageThumbs { count: total, thumbs })
}

#[tauri::command]
pub async fn pdf_page_thumbs(path: String) -> AppResult<PageThumbs> {
    let joined = tauri::async_runtime::spawn_blocking(move || page_thumbs_blocking(path)).await;
    match joined {
        Ok(inner) => inner,
        Err(e) => Err(AppError::unknown(e)),
    }
}
