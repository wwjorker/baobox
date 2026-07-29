use crate::batch::{run_batch, FileOutcome};
use crate::err::{AppError, AppResult};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 图片水印
///
/// 复用了 PDF 那边找系统中文字体的思路：字体不随包分发（微软雅黑受版权
/// 保护），运行时从系统读。区别是这里要自己把字形光栅化到像素上，
/// 而不是嵌进 PDF 让阅读器去画。

const CANDIDATES: &[&str] = &[
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\simhei.ttf",
    r"C:\Windows\Fonts\arial.ttf",
];

fn load_font_bytes() -> AppResult<Vec<u8>> {
    for p in CANDIDATES {
        if let Ok(d) = std::fs::read(p) {
            return Ok(d);
        }
    }
    Err(AppError::new("err.fontMissing"))
}

/// 把一行文字画到图上，返回是否成功
fn draw_text(
    img: &mut image::RgbaImage,
    font: &FontRef,
    text: &str,
    size: f32,
    ox: f32,
    oy: f32,
    alpha: f32,
    dark: bool,
) {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut pen_x = ox;
    let base = oy + scaled.ascent();

    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        let adv = scaled.h_advance(gid);
        let glyph = gid.with_scale_and_position(size, ab_glyph::point(pen_x, base));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                    return;
                }
                let a = cov * alpha;
                if a <= 0.004 {
                    return;
                }
                let old = img.get_pixel(px as u32, py as u32).0;
                // 水印要能透出底下的内容，所以做的是混合而不是覆盖
                let ink = if dark { 0.0 } else { 255.0 };
                let mix = |o: u8| (o as f32 * (1.0 - a) + ink * a) as u8;
                img.put_pixel(
                    px as u32,
                    py as u32,
                    image::Rgba([mix(old[0]), mix(old[1]), mix(old[2]), old[3].max(200)]),
                );
            });
        }
        pen_x += adv;
    }
}

fn text_width(font: &FontRef, text: &str, size: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    text.chars().map(|c| scaled.h_advance(font.glyph_id(c))).sum()
}

pub fn watermark_file(
    src: &Path,
    text: &str,
    opacity: f32,
    tile: bool,
) -> AppResult<(PathBuf, u32)> {
    if text.trim().is_empty() {
        return Err(AppError::new("err.stampEmpty"));
    }
    let bytes = std::fs::read(long_path(src))?;
    let mut img = image::load_from_memory(&bytes)
        .map_err(|e| AppError::decode("图片", e))?
        .to_rgba8();

    let font_bytes = load_font_bytes()?;
    // TTC 集合取第一个字体
    let font = FontRef::try_from_slice(&font_bytes)
        .or_else(|_| FontRef::try_from_slice_and_index(&font_bytes, 0))
        .map_err(|e| AppError::new("err.fontMissing").detail(format!("{e:?}")))?;

    let (w, h) = (img.width() as f32, img.height() as f32);
    // 字号跟着图片走，否则大图上的水印小得看不见，小图上又糊满整张
    let size = (w.min(h) / 14.0).clamp(14.0, 96.0);
    let tw = text_width(&font, text, size);
    // 亮底用深色字，暗底用浅色字
    let dark = average_luma(&img) > 128.0;
    let mut count = 0u32;

    if tile {
        // 平铺：斜向重复，覆盖整张。截图外传时这种最难被裁掉。
        let step_x = (tw + size * 2.0).max(size * 4.0);
        let step_y = size * 4.0;
        let mut row = 0;
        let mut y = -size;
        while y < h + size {
            let offset = if row % 2 == 0 { 0.0 } else { step_x / 2.0 };
            let mut x = -tw + offset;
            while x < w {
                draw_text(&mut img, &font, text, size, x, y, opacity, dark);
                count += 1;
                x += step_x;
            }
            y += step_y;
            row += 1;
        }
    } else {
        // 单个：右下角，留出边距
        let pad = size * 0.6;
        draw_text(
            &mut img,
            &font,
            text,
            size,
            w - tw - pad,
            h - size * 1.6,
            opacity,
            dark,
        );
        count = 1;
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 加水印", stem_of(src)), "png");
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AppError::unknown(e))?;
    std::fs::write(long_path(&dst), out.into_inner())?;
    Ok((dst, count))
}

fn average_luma(img: &image::RgbaImage) -> f32 {
    // 抽样估算即可，全图逐像素对大图太浪费
    let step = (img.width().max(img.height()) / 200).max(1);
    let (mut sum, mut n) = (0f32, 0f32);
    for y in (0..img.height()).step_by(step as usize) {
        for x in (0..img.width()).step_by(step as usize) {
            let p = img.get_pixel(x, y).0;
            sum += 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
            n += 1.0;
        }
    }
    if n == 0.0 { 128.0 } else { sum / n }
}

fn img_watermark_blocking(
    app: AppHandle,
    paths: Vec<String>,
    text: String,
    opacity: u8,
    tile: bool,
) -> Vec<FileOutcome> {
    let a = (opacity as f32 / 100.0).clamp(0.03, 1.0);
    run_batch(&app, paths, move |src| {
        let (dst, n) = watermark_file(src, &text, a, tile)?;
        Ok((dst, Some(format!("绘制 {n} 处"))))
    })
}

#[tauri::command]
pub async fn img_watermark(
    app: AppHandle,
    paths: Vec<String>,
    text: String,
    opacity: u8,
    tile: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        img_watermark_blocking(app, paths, text, opacity, tile)
    })
    .await
    .unwrap_or_default()
}
