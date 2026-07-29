//! 图片编辑类工具。
//!
//! 跟 image_ops 分开：那边管的是「同一张图，换个编码」——压缩、转格式、缩放；
//! 这边改的是画面本身——切开、拼起、裁掉、上色。两类的失败模式和参数
//! 完全不一样，混在一个文件里只会越来越难读。
//!
//! 这一组里九宫格和长图拼接是特意先做的：中文社区天天在用（朋友圈九宫格、
//! 聊天记录长截图），而欧美的图片工具普遍不提供，是当初定的差异化点之一。

use crate::batch::{run_batch, FileOutcome, Note};
use crate::err::{AppError, AppResult};
use crate::image_ops::{encode, load, write_out, OutFmt};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use image::{DynamicImage, GenericImage, GenericImageView, Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

// ================================================================ 九宫格切图

/// 把一张图切成 rows×cols 块。
///
/// `square_first` 为真时先按中心裁成正方形——朋友圈九宫格要的是九个方块，
/// 直接按原比例切长方形，发出去拼起来是歪的。
pub fn grid_file(
    src: &Path,
    rows: u32,
    cols: u32,
    square_first: bool,
) -> AppResult<(PathBuf, u32)> {
    let img = load(src)?;
    let img = if square_first { center_square(&img) } else { img };
    let (w, h) = img.dimensions();

    if w < cols || h < rows {
        return Err(AppError::new("err.tooSmallToSplit"));
    }

    let dir = output_dir_for(src)?;
    let stem = stem_of(src);
    let fmt = OutFmt::Keep.resolve(src);
    // 整除不尽时余数给最后一行/列，总比整体裁掉一条好
    let tile_w = w / cols;
    let tile_h = h / rows;
    let mut last = dir.clone();

    for r in 0..rows {
        for c in 0..cols {
            let x = c * tile_w;
            let y = r * tile_h;
            let tw = if c == cols - 1 { w - x } else { tile_w };
            let th = if r == rows - 1 { h - y } else { tile_h };
            let tile = img.crop_imm(x, y, tw, th);
            let data = encode(&tile, fmt, 92)?;
            // 编号从 1 开始按行排，正好是九宫格发图的顺序
            let n = r * cols + c + 1;
            let dst = unique_path(&dir, &format!("{stem}_{n:02}"), fmt.ext());
            std::fs::write(long_path(&dst), &data)?;
            last = dst;
        }
    }
    Ok((last, rows * cols))
}

/// 按中心裁成正方形，短边为准
fn center_square(img: &DynamicImage) -> DynamicImage {
    let (w, h) = img.dimensions();
    let side = w.min(h);
    img.crop_imm((w - side) / 2, (h - side) / 2, side, side)
}

fn img_grid_blocking(
    app: AppHandle,
    paths: Vec<String>,
    rows: u32,
    cols: u32,
    square_first: bool,
) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let (last, n) = grid_file(src, rows, cols, square_first)?;
        Ok((
            last,
            Some(Note::new("note.grid").with("rows", rows).with("cols", cols).with("n", n)),
        ))
    })
}

#[tauri::command]
pub async fn img_grid(
    app: AppHandle,
    paths: Vec<String>,
    rows: u32,
    cols: u32,
    square_first: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        img_grid_blocking(app, paths, rows.clamp(1, 10), cols.clamp(1, 10), square_first)
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 长图拼接

/// 把多张图接成一张长图。
///
/// 纵向时把所有图缩放到同一宽度再接，否则宽窄不一的截图拼出来两边是锯齿状的。
/// 宽度取最窄的那张——放大会糊，缩小不会。
pub fn stitch(srcs: &[PathBuf], vertical: bool, gap: u32) -> AppResult<(PathBuf, u32)> {
    if srcs.is_empty() {
        return Err(AppError::new("err.needAtLeastTwo"));
    }
    let imgs: Vec<DynamicImage> = srcs.iter().map(|p| load(p)).collect::<AppResult<_>>()?;

    // 统一到最小的那个尺寸，只缩不放
    let scaled: Vec<DynamicImage> = if vertical {
        let target_w = imgs.iter().map(|i| i.width()).min().unwrap_or(1).max(1);
        imgs.iter()
            .map(|i| {
                if i.width() == target_w {
                    i.clone()
                } else {
                    let h = (i.height() as f64 * target_w as f64 / i.width() as f64).round() as u32;
                    i.resize_exact(target_w, h.max(1), image::imageops::FilterType::Lanczos3)
                }
            })
            .collect()
    } else {
        let target_h = imgs.iter().map(|i| i.height()).min().unwrap_or(1).max(1);
        imgs.iter()
            .map(|i| {
                if i.height() == target_h {
                    i.clone()
                } else {
                    let w = (i.width() as f64 * target_h as f64 / i.height() as f64).round() as u32;
                    i.resize_exact(w.max(1), target_h, image::imageops::FilterType::Lanczos3)
                }
            })
            .collect()
    };

    let gaps = gap * (scaled.len().saturating_sub(1)) as u32;
    let (out_w, out_h) = if vertical {
        (
            scaled[0].width(),
            scaled.iter().map(|i| i.height()).sum::<u32>() + gaps,
        )
    } else {
        (
            scaled.iter().map(|i| i.width()).sum::<u32>() + gaps,
            scaled[0].height(),
        )
    };

    // 长截图很容易撞到几万像素，先挡住，别等分配内存时才崩
    const MAX_SIDE: u32 = 30_000;
    if out_w > MAX_SIDE || out_h > MAX_SIDE {
        return Err(AppError::new("err.stitchTooLong")
            .var("max", MAX_SIDE.to_string())
            .var("got", out_w.max(out_h).to_string()));
    }

    // 间隙填白，透明会在 JPEG 下变黑
    let mut canvas = RgbaImage::from_pixel(out_w, out_h, Rgba([255, 255, 255, 255]));
    let mut cursor = 0u32;
    for img in &scaled {
        let rgba = img.to_rgba8();
        if vertical {
            canvas
                .copy_from(&rgba, 0, cursor)
                .map_err(|e| AppError::unknown(e))?;
            cursor += img.height() + gap;
        } else {
            canvas
                .copy_from(&rgba, cursor, 0)
                .map_err(|e| AppError::unknown(e))?;
            cursor += img.width() + gap;
        }
    }

    let first = &srcs[0];
    let dir = output_dir_for(first)?;
    let fmt = OutFmt::Keep.resolve(first);
    let out = DynamicImage::ImageRgba8(canvas);
    let data = encode(&out, fmt, 92)?;
    let dst = unique_path(
        &dir,
        &format!("{} 等 {} 张拼接", stem_of(first), srcs.len()),
        fmt.ext(),
    );
    std::fs::write(long_path(&dst), &data)?;
    Ok((dst, scaled.len() as u32))
}

#[tauri::command]
pub async fn img_stitch(
    app: AppHandle,
    paths: Vec<String>,
    direction: String,
    gap: u32,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        if paths.is_empty() {
            return Vec::new();
        }
        let srcs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let vertical = direction != "horizontal";
        let total = srcs.len();

        let o = match stitch(&srcs, vertical, gap.min(200)) {
            Ok((dst, n)) => FileOutcome::ok(&srcs[0], dst, Some(Note::new("note.stitched").with("n", n))),
            Err(e) => FileOutcome::fail(&srcs[0], e),
        };
        // 同合并：产物一份挂第一个，其余各发一条「已并入」
        let outcomes = crate::batch::fold_outcomes(o, &srcs[1..]);
        for (i, o) in outcomes.iter().enumerate() {
            crate::batch::emit(&app, i, total, o);
        }
        outcomes
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 去白边

/// 裁掉四周的纯色边。
///
/// 扫描四条边，逐行/逐列判断是否与角落像素足够接近。截图、扫描件、
/// 从 PPT 导出的图常常带一圈白边，手动裁一百张不现实。
pub fn trim_file(src: &Path, tolerance: u8) -> AppResult<(PathBuf, u32, u32, u32, u32)> {
    let img = load(src)?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return Err(AppError::new("err.decode").var("format", "图片"));
    }

    // 以左上角像素为基准色。用角落而不是「白」——深色背景的图同样该被裁。
    let base = *rgba.get_pixel(0, 0);
    let tol = tolerance as i32;
    let near = |p: &Rgba<u8>| -> bool {
        // 透明像素一律视作可裁的边
        if p.0[3] == 0 && base.0[3] == 0 {
            return true;
        }
        (0..3).all(|i| (p.0[i] as i32 - base.0[i] as i32).abs() <= tol)
            && (p.0[3] as i32 - base.0[3] as i32).abs() <= tol
    };

    let row_uniform = |y: u32| (0..w).all(|x| near(rgba.get_pixel(x, y)));
    let col_uniform = |x: u32| (0..h).all(|y| near(rgba.get_pixel(x, y)));

    let mut top = 0;
    while top < h && row_uniform(top) {
        top += 1;
    }
    // 整张都是同一个色，没什么可裁的
    if top == h {
        return Err(AppError::new("err.trimAllUniform"));
    }
    let mut bottom = h - 1;
    while bottom > top && row_uniform(bottom) {
        bottom -= 1;
    }
    let mut left = 0;
    while left < w && col_uniform(left) {
        left += 1;
    }
    let mut right = w - 1;
    while right > left && col_uniform(right) {
        right -= 1;
    }

    let cropped = img.crop_imm(left, top, right - left + 1, bottom - top + 1);
    let (nw, nh) = cropped.dimensions();
    let fmt = OutFmt::Keep.resolve(src);
    let data = encode(&cropped, fmt, 92)?;
    let dst = write_out(src, fmt, &data)?;
    Ok((dst, w, h, nw, nh))
}

#[tauri::command]
pub async fn img_trim(app: AppHandle, paths: Vec<String>, tolerance: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let tol = tolerance.min(255) as u8;
        run_batch(&app, paths, move |src| {
            let (dst, w, h, nw, nh) = trim_file(src, tol)?;
            Ok((
                dst,
                Some(
                    Note::new("note.trimmed")
                        .with("w", w)
                        .with("h", h)
                        .with("nw", nw)
                        .with("nh", nh),
                ),
            ))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 圆角与边框

/// 加圆角和外边框。
///
/// 圆角靠 alpha 实现，所以一律输出 PNG——存成 JPEG 圆角会被填成黑色或白色，
/// 等于白做。这一点在结果说明里也告诉用户。
pub fn frame_file(
    src: &Path,
    radius_pct: u32,
    border: u32,
    dark: bool,
) -> AppResult<(PathBuf, u32)> {
    let img = load(src)?;
    let (w, h) = img.dimensions();
    let src_rgba = img.to_rgba8();

    // 半径按短边百分比，这样不同尺寸的图看起来圆得一致
    let radius = (w.min(h) as f32 * radius_pct as f32 / 100.0).round() as u32;
    let border_color = if dark {
        Rgba([20, 17, 9, 255])
    } else {
        Rgba([255, 255, 255, 255])
    };

    let out_w = w + border * 2;
    let out_h = h + border * 2;
    let mut canvas = RgbaImage::from_pixel(out_w, out_h, Rgba([0, 0, 0, 0]));

    for y in 0..out_h {
        for x in 0..out_w {
            // 外圈也跟着圆，否则圆角图片外面套个方边框，很怪
            if !inside_rounded(x, y, out_w, out_h, radius + border) {
                continue;
            }
            let inner_x = x as i64 - border as i64;
            let inner_y = y as i64 - border as i64;
            let in_bounds = inner_x >= 0 && inner_y >= 0 && (inner_x as u32) < w && (inner_y as u32) < h;
            if in_bounds && inside_rounded(inner_x as u32, inner_y as u32, w, h, radius) {
                canvas.put_pixel(x, y, *src_rgba.get_pixel(inner_x as u32, inner_y as u32));
            } else if border > 0 {
                canvas.put_pixel(x, y, border_color);
            }
        }
    }

    let out = DynamicImage::ImageRgba8(canvas);
    let data = encode(&out, OutFmt::Png, 100)?;
    let dst = write_out(src, OutFmt::Png, &data)?;
    Ok((dst, radius))
}

/// 这个点在圆角矩形里面吗
fn inside_rounded(x: u32, y: u32, w: u32, h: u32, r: u32) -> bool {
    if r == 0 {
        return true;
    }
    let r = r.min(w / 2).min(h / 2) as f32;
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    // 只有四个角需要判圆，中间的十字区域一律在内
    let cx = if fx < r {
        r
    } else if fx > w as f32 - r {
        w as f32 - r
    } else {
        return true;
    };
    let cy = if fy < r {
        r
    } else if fy > h as f32 - r {
        h as f32 - r
    } else {
        return true;
    };
    let (dx, dy) = (fx - cx, fy - cy);
    dx * dx + dy * dy <= r * r
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn img_frame(
    app: AppHandle,
    paths: Vec<String>,
    radius: u32,
    border: u32,
    borderDark: bool,
) -> Vec<FileOutcome> {
    let dark = borderDark;
    tauri::async_runtime::spawn_blocking(move || {
        let r = radius.min(50);
        let b = border.min(200);
        run_batch(&app, paths, move |src| {
            let (dst, px) = frame_file(src, r, b, dark)?;
            Ok((dst, Some(Note::new("note.framed").with("px", px))))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 调色

/// 亮度 / 对比度 / 饱和度 / 灰度。
///
/// 四个都做成一个工具而不是四个：真实使用里几乎总是一起调的，
/// 拆开等于逼用户跑四遍，每遍都重新编码一次。
pub fn adjust_file(
    src: &Path,
    brightness: i32,
    contrast: i32,
    saturation: i32,
    gray: bool,
) -> AppResult<PathBuf> {
    let img = load(src)?;
    let mut work = img;

    if brightness != 0 {
        work = work.brighten(brightness);
    }
    if contrast != 0 {
        work = work.adjust_contrast(contrast as f32);
    }
    if gray {
        work = DynamicImage::ImageLuma8(work.to_luma8());
    } else if saturation != 0 {
        work = apply_saturation(&work, saturation);
    }

    let fmt = OutFmt::Keep.resolve(src);
    let data = encode(&work, fmt, 92)?;
    write_out(src, fmt, &data)
}

/// image crate 没有饱和度调节，按亮度做线性插值即可：
/// 系数 0 是灰度，1 是原样，大于 1 是加饱和。
fn apply_saturation(img: &DynamicImage, amount: i32) -> DynamicImage {
    let factor = 1.0 + amount as f32 / 100.0;
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        // Rec. 601 亮度权重，与人眼感知一致
        let lum = 0.299 * p.0[0] as f32 + 0.587 * p.0[1] as f32 + 0.114 * p.0[2] as f32;
        for i in 0..3 {
            p.0[i] = (lum + (p.0[i] as f32 - lum) * factor).clamp(0.0, 255.0) as u8;
        }
    }
    DynamicImage::ImageRgba8(rgba)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn img_adjust(
    app: AppHandle,
    paths: Vec<String>,
    brightness: i32,
    contrast: i32,
    saturation: i32,
    grayscale: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let dst = adjust_file(src, brightness, contrast, saturation, grayscale)?;
            Ok((
                dst,
                Some(if grayscale {
                    Note::new("note.grayscaled")
                } else {
                    Note::new("note.adjusted")
                        .with("b", brightness)
                        .with("c", contrast)
                        .with("s", saturation)
                }),
            ))
        })
    })
    .await
    .unwrap_or_default()
}
