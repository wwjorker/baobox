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

// ================================================================ 按比例裁切

/// 统一裁成同一个画幅比例。
///
/// 电商主图要 1:1、视频封面要 16:9、小红书要 3:4——一批照片来自不同设备，
/// 比例参差不齐，上传后被平台自动裁一刀，裁掉的往往正是主体。
/// 自己先按中心裁好，至少知道保住了什么。
pub fn aspect_file(src: &Path, ratio: &str) -> AppResult<(PathBuf, u32, u32, u32, u32)> {
    let img = load(src)?;
    let (w, h) = img.dimensions();

    let (rw, rh): (f32, f32) = match ratio {
        "1:1" => (1.0, 1.0),
        "4:3" => (4.0, 3.0),
        "3:4" => (3.0, 4.0),
        "16:9" => (16.0, 9.0),
        "9:16" => (9.0, 16.0),
        _ => (1.0, 1.0),
    };
    let target = rw / rh;
    let current = w as f32 / h as f32;

    // 只裁不填：补白条等于把画面推小，多数人要的是裁
    let (nw, nh) = if current > target {
        ((h as f32 * target).round() as u32, h)
    } else {
        (w, (w as f32 / target).round() as u32)
    };
    let (nw, nh) = (nw.max(1).min(w), nh.max(1).min(h));

    let cropped = img.crop_imm((w - nw) / 2, (h - nh) / 2, nw, nh);
    let fmt = OutFmt::Keep.resolve(src);
    let data = encode(&cropped, fmt, 92)?;
    let dst = write_out(src, fmt, &data)?;
    Ok((dst, w, h, nw, nh))
}

#[tauri::command]
pub async fn img_aspect(app: AppHandle, paths: Vec<String>, ratio: String) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let (dst, w, h, nw, nh) = aspect_file(src, &ratio)?;
            Ok((
                dst,
                Some(
                    Note::new("note.aspect")
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

// ================================================================ 主色调提取

/// 数出图里最主要的几个颜色。
///
/// 做配图、选背景色、给相册分色时要用。按 5 位精度分桶统计——
/// 全精度统计等于每个像素一个桶，照片上根本聚不出「主色」。
pub fn palette_of(src: &Path, count: usize) -> AppResult<Vec<(String, f32)>> {
    let img = load(src)?;
    // 大图先缩小，主色调跟分辨率无关，全尺寸统计纯属浪费
    let small = img.thumbnail(200, 200).to_rgb8();

    let mut buckets: std::collections::HashMap<(u8, u8, u8), u32> = std::collections::HashMap::new();
    let mut total = 0u32;
    for p in small.pixels() {
        // 每通道压到 32 级，相近的颜色才会落进同一个桶
        let key = (p.0[0] >> 3, p.0[1] >> 3, p.0[2] >> 3);
        *buckets.entry(key).or_insert(0) += 1;
        total += 1;
    }

    let mut list: Vec<((u8, u8, u8), u32)> = buckets.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1));

    Ok(list
        .into_iter()
        .take(count)
        .map(|((r, g, b), n)| {
            // 回到桶的中心值，直接左移会让所有颜色都偏暗
            let (r, g, b) = ((r << 3) | 4, (g << 3) | 4, (b << 3) | 4);
            (
                format!("#{r:02X}{g:02X}{b:02X}"),
                n as f32 * 100.0 / total.max(1) as f32,
            )
        })
        .collect())
}

#[tauri::command]
pub async fn img_palette(app: AppHandle, paths: Vec<String>, count: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let n = count.clamp(1, 12) as usize;
        let total = paths.len();
        let mut out = Vec::with_capacity(total);
        crate::batch::reset_cancel();

        for (index, p) in paths.iter().enumerate() {
            let src = PathBuf::from(p);
            if crate::batch::cancelled() {
                let o = FileOutcome::skipped(&src, "run.cancelledSkip");
                crate::batch::emit(&app, index, total, &o);
                out.push(o);
                continue;
            }
            let o = match palette_of(&src, n) {
                Ok(list) => {
                    let text = list
                        .iter()
                        .map(|(hex, pct)| format!("{hex}  {pct:.1}%"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    FileOutcome::text_only(
                        &src,
                        text,
                        Some(Note::new("note.paletteFound").with("n", list.len())),
                    )
                }
                Err(e) => FileOutcome::fail(&src, e),
            };
            crate::batch::emit(&app, index, total, &o);
            out.push(o);
        }
        out
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 转 Base64

/// 图片转 data URI。
///
/// 写网页时把小图直接内联进 HTML/CSS，省一次请求；也常用于把图贴进
/// 只接受文本的地方。产物同时落一份 txt，因为几十 KB 的字符串
/// 没法靠界面里那个框读完。
pub fn base64_of(src: &Path) -> AppResult<(PathBuf, String, usize)> {
    use base64::Engine;
    let bytes = std::fs::read(long_path(src))?;
    let mime = match src
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => "image/jpeg",
    };
    let uri = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &stem_of(src), "txt");
    std::fs::write(long_path(&dst), uri.as_bytes())?;
    Ok((dst, uri, bytes.len()))
}

#[tauri::command]
pub async fn img_base64(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let total = paths.len();
        let mut out = Vec::with_capacity(total);
        crate::batch::reset_cancel();

        for (index, p) in paths.iter().enumerate() {
            let src = PathBuf::from(p);
            if crate::batch::cancelled() {
                let o = FileOutcome::skipped(&src, "run.cancelledSkip");
                crate::batch::emit(&app, index, total, &o);
                out.push(o);
                continue;
            }
            let o = match base64_of(&src) {
                Ok((dst, uri, _)) => {
                    let mut o = FileOutcome::text_only(
                        &src,
                        uri.clone(),
                        Some(Note::new("note.base64").with("kb", uri.len() / 1024)),
                    );
                    o.out_path = Some(dst.to_string_lossy().to_string());
                    o
                }
                Err(e) => FileOutcome::fail(&src, e),
            };
            crate::batch::emit(&app, index, total, &o);
            out.push(o);
        }
        out
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 画布扩展

/// 补边而不裁，把画面撑到指定比例。
///
/// 跟「按比例裁切」正好相反：那个会切掉画面，这个一个像素都不丢，
/// 靠加边补齐。发商品图、做幻灯片配图时常要求统一尺寸又不许裁到主体。
pub fn expand_file(src: &Path, ratio: &str, dark: bool) -> AppResult<(PathBuf, u32, u32)> {
    let img = load(src)?;
    let (w, h) = img.dimensions();

    let (rw, rh): (f32, f32) = match ratio {
        "1:1" => (1.0, 1.0),
        "4:3" => (4.0, 3.0),
        "3:4" => (3.0, 4.0),
        "16:9" => (16.0, 9.0),
        "9:16" => (9.0, 16.0),
        _ => (1.0, 1.0),
    };
    let target = rw / rh;
    let current = w as f32 / h as f32;

    // 只加不减：哪个方向不够就补哪个方向
    let (nw, nh) = if current > target {
        (w, (w as f32 / target).round() as u32)
    } else {
        ((h as f32 * target).round() as u32, h)
    };
    let (nw, nh) = (nw.max(w), nh.max(h));

    let fill = if dark {
        Rgba([20, 17, 9, 255])
    } else {
        Rgba([255, 255, 255, 255])
    };
    let mut canvas = RgbaImage::from_pixel(nw, nh, fill);
    canvas
        .copy_from(&img.to_rgba8(), (nw - w) / 2, (nh - h) / 2)
        .map_err(|e| AppError::unknown(e))?;

    let out = DynamicImage::ImageRgba8(canvas);
    let fmt = OutFmt::Keep.resolve(src);
    let data = encode(&out, fmt, 92)?;
    let dst = write_out(src, fmt, &data)?;
    Ok((dst, nw, nh))
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn img_expand(
    app: AppHandle,
    paths: Vec<String>,
    ratio: String,
    fillDark: bool,
) -> Vec<FileOutcome> {
    let dark = fillDark;
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let (dst, nw, nh) = expand_file(src, &ratio, dark)?;
            Ok((dst, Some(Note::new("note.expanded").with("nw", nw).with("nh", nh))))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ GIF 拆帧

/// 把动图拆成一张张图片。
///
/// 想拿其中某一帧做封面、或者只改动图里的一帧再拼回去，都得先拆开。
pub fn gif_frames(src: &Path, every: u32) -> AppResult<(PathBuf, usize, usize)> {
    use image::AnimationDecoder;

    let file = std::fs::File::open(long_path(src))?;
    let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file))
        .map_err(|e| AppError::decode("GIF", e))?;
    let frames: Vec<_> = decoder
        .into_frames()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::decode("GIF", e))?;
    if frames.is_empty() {
        return Err(AppError::new("err.gifNoFrames"));
    }

    let dir = output_dir_for(src)?;
    let stem = stem_of(src);
    let step = every.max(1) as usize;
    let mut saved = 0usize;
    let mut last = dir.clone();

    for (i, frame) in frames.iter().enumerate() {
        if i % step != 0 {
            continue;
        }
        let buf = frame.buffer();
        let dyn_img = DynamicImage::ImageRgba8(buf.clone());
        // 帧可能带透明，一律存 PNG
        let data = encode(&dyn_img, OutFmt::Png, 100)?;
        let dst = unique_path(&dir, &format!("{stem}_帧{:03}", i + 1), "png");
        std::fs::write(long_path(&dst), &data)?;
        last = dst;
        saved += 1;
    }
    Ok((last, frames.len(), saved))
}

#[tauri::command]
pub async fn gif_split(app: AppHandle, paths: Vec<String>, every: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let (last, total, saved) = gif_frames(src, every)?;
            Ok((
                last,
                Some(Note::new("note.gifSplit").with("total", total).with("saved", saved)),
            ))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ GIF 制作

/// 一批图做成动图。
///
/// 顺序就是列表顺序。所有帧统一到第一张的尺寸——GIF 规范要求所有帧
/// 共用一个画布，尺寸不一致的话要么报错要么错位。
pub fn make_gif(srcs: &[PathBuf], delay_ms: u32) -> AppResult<(PathBuf, usize)> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::Delay;

    if srcs.len() < 2 {
        return Err(AppError::new("err.gifNeedTwo"));
    }
    let first = load(&srcs[0])?;
    let (w, h) = first.dimensions();

    let dir = output_dir_for(&srcs[0])?;
    let dst = unique_path(&dir, &format!("{} 等 {} 帧", stem_of(&srcs[0]), srcs.len()), "gif");

    let out = std::fs::File::create(long_path(&dst))?;
    let mut enc = GifEncoder::new_with_speed(std::io::BufWriter::new(out), 10);
    enc.set_repeat(Repeat::Infinite)
        .map_err(|e| AppError::unknown(e))?;

    let delay = Delay::from_numer_denom_ms(delay_ms.clamp(20, 5000), 1);
    let mut n = 0usize;
    for p in srcs {
        let img = load(p)?;
        let sized = if img.dimensions() == (w, h) {
            img
        } else {
            img.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        };
        let frame = image::Frame::from_parts(sized.to_rgba8(), 0, 0, delay);
        enc.encode_frame(frame).map_err(|e| AppError::unknown(e))?;
        n += 1;
    }
    drop(enc);
    Ok((dst, n))
}

#[tauri::command]
pub async fn gif_make(app: AppHandle, paths: Vec<String>, delay: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        if paths.is_empty() {
            return Vec::new();
        }
        let srcs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let total = srcs.len();
        let o = match make_gif(&srcs, delay) {
            Ok((dst, n)) => {
                FileOutcome::ok(&srcs[0], dst, Some(Note::new("note.gifMade").with("n", n)))
            }
            Err(e) => FileOutcome::fail(&srcs[0], e),
        };
        // 同拼接：产物一份挂第一个，其余各发一条「已并入」
        let outcomes = crate::batch::fold_outcomes(o, &srcs[1..]);
        for (i, o) in outcomes.iter().enumerate() {
            crate::batch::emit(&app, i, total, o);
        }
        outcomes
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 生成 ICO

/// 网站图标和 Windows 程序图标要的都是一个 .ico 里装多个尺寸。
///
/// 容器自己写：结构就是 6 字节文件头 + 每尺寸 16 字节目录项 + 各自的 PNG，
/// 比为它去挑一个库更省事，也能确切控制装哪几个尺寸。
/// 256 及以下全部用 PNG 载荷（Vista 起支持），不用老的 BMP 格式。
const ICO_SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

pub fn ico_file(src: &Path) -> AppResult<(PathBuf, usize)> {
    let img = load(src)?;
    // 先按中心裁成正方形，否则图标会被压扁
    let square = center_square(&img);

    let mut payloads: Vec<(u32, Vec<u8>)> = Vec::new();
    for size in ICO_SIZES {
        // 只缩不放：原图比这个尺寸还小的话，放大出来是糊的
        if square.width() < size && size != ICO_SIZES[0] {
            continue;
        }
        let small = square.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        let mut buf = std::io::Cursor::new(Vec::new());
        small
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| AppError::unknown(e))?;
        payloads.push((size, buf.into_inner()));
    }
    if payloads.is_empty() {
        return Err(AppError::new("err.decode").var("format", "图片"));
    }

    let n = payloads.len();
    let mut out = Vec::new();
    // 文件头：保留位 0、类型 1（图标）、图像数量
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(n as u16).to_le_bytes());

    let mut offset = 6 + 16 * n as u32;
    for (size, data) in &payloads {
        // 256 在这一字节里写 0——字段只有 8 位，装不下 256
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(0); // 调色板数
        out.push(0); // 保留
        out.extend_from_slice(&1u16.to_le_bytes()); // 颜色平面
        out.extend_from_slice(&32u16.to_le_bytes()); // 位深
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &payloads {
        out.extend_from_slice(data);
    }

    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &stem_of(src), "ico");
    std::fs::write(long_path(&dst), &out)?;
    Ok((dst, n))
}

#[tauri::command]
pub async fn img_ico(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, |src| {
            let (dst, n) = ico_file(src)?;
            Ok((dst, Some(Note::new("note.icoMade").with("n", n))))
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
    filter: &str,
) -> AppResult<PathBuf> {
    let img = load(src)?;
    let mut work = img;

    if brightness != 0 {
        work = work.brighten(brightness);
    }
    if contrast != 0 {
        work = work.adjust_contrast(contrast as f32);
    }

    match filter {
        "gray" => work = DynamicImage::ImageLuma8(work.to_luma8()),
        "sepia" => work = apply_sepia(&work),
        "invert" => {
            let mut rgba = work.to_rgba8();
            for p in rgba.pixels_mut() {
                for i in 0..3 {
                    p.0[i] = 255 - p.0[i];
                }
            }
            work = DynamicImage::ImageRgba8(rgba);
        }
        _ => {
            if saturation != 0 {
                work = apply_saturation(&work, saturation);
            }
        }
    }

    let fmt = OutFmt::Keep.resolve(src);
    let data = encode(&work, fmt, 92)?;
    write_out(src, fmt, &data)
}

// ================================================================ 自动色阶

/// 自动色阶：把最暗和最亮拉到纯黑纯白，中间线性铺开。
///
/// 阴天拍的、翻拍的、扫描的图往往灰蒙蒙一片没有对比。取每个通道的
/// 直方图，掐掉两端各 0.5% 的极端像素（否则一个亮点就把白点顶满、
/// 白拉不动），再把剩下的范围拉到 0–255。
pub fn autolevel_file(src: &Path) -> AppResult<PathBuf> {
    let img = load(src)?;
    let mut rgba = img.to_rgba8();

    // 逐通道算裁剪点。分开算而不是按灰度统一算——偏色的图
    // （比如泛黄的旧照）分通道拉才能把色偏一并纠掉。
    let (w, h) = rgba.dimensions();
    let total = (w * h) as u32;
    let clip = (total as f32 * 0.005) as u32; // 两端各掐 0.5%

    let mut lut = [[0u8; 256]; 3];
    for ch in 0..3 {
        let mut hist = [0u32; 256];
        for p in rgba.pixels() {
            hist[p.0[ch] as usize] += 1;
        }
        // 从两头往中间累加，越过 clip 的位置就是新的黑点/白点
        let mut lo = 0usize;
        let mut acc = 0u32;
        while lo < 255 && acc + hist[lo] <= clip {
            acc += hist[lo];
            lo += 1;
        }
        let mut hi = 255usize;
        acc = 0;
        while hi > lo && acc + hist[hi] <= clip {
            acc += hist[hi];
            hi -= 1;
        }
        let span = (hi - lo).max(1) as f32;
        for (v, slot) in lut[ch].iter_mut().enumerate() {
            let mapped = ((v as f32 - lo as f32) / span * 255.0).round();
            *slot = mapped.clamp(0.0, 255.0) as u8;
        }
    }

    for p in rgba.pixels_mut() {
        for ch in 0..3 {
            p.0[ch] = lut[ch][p.0[ch] as usize];
        }
    }

    let out = DynamicImage::ImageRgba8(rgba);
    let fmt = OutFmt::Keep.resolve(src);
    let data = encode(&out, fmt, 92)?;
    write_out(src, fmt, &data)
}

#[tauri::command]
pub async fn img_autolevel(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, |src| {
            let dst = autolevel_file(src)?;
            Ok((dst, Some(Note::new("note.autoleveled"))))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 锐化

/// 非锐化掩模：先高斯模糊，再按原图与模糊图的差值加回去。
///
/// 缩小后的图、扫描件常发虚，锐化能把边缘找回来。用非锐化掩模而不是
/// 简单的卷积核，是因为它对噪点更温和——只强化真正的边，不放大平坦区的颗粒。
pub fn sharpen_file(src: &Path, amount: i32) -> AppResult<PathBuf> {
    let img = load(src)?;
    // image 自带 unsharpen：sigma 固定，threshold 控制「差多少才算边」。
    // amount 映射到 sigma 强度，0–100 → 0.5–3.0
    let sigma = 0.5 + amount.clamp(0, 100) as f32 / 100.0 * 2.5;
    let sharpened = img.unsharpen(sigma, 3);

    let fmt = OutFmt::Keep.resolve(src);
    let data = encode(&sharpened, fmt, 92)?;
    write_out(src, fmt, &data)
}

#[tauri::command]
pub async fn img_sharpen(app: AppHandle, paths: Vec<String>, amount: i32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let dst = sharpen_file(src, amount)?;
            Ok((dst, Some(Note::new("note.sharpened").with("n", amount))))
        })
    })
    .await
    .unwrap_or_default()
}

/// 棕褐色调。系数用的是通行的那组，效果跟老照片的观感对得上。
fn apply_sepia(img: &DynamicImage) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let (r, g, b) = (p.0[0] as f32, p.0[1] as f32, p.0[2] as f32);
        p.0[0] = (0.393 * r + 0.769 * g + 0.189 * b).min(255.0) as u8;
        p.0[1] = (0.349 * r + 0.686 * g + 0.168 * b).min(255.0) as u8;
        p.0[2] = (0.272 * r + 0.534 * g + 0.131 * b).min(255.0) as u8;
    }
    DynamicImage::ImageRgba8(rgba)
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
pub async fn img_adjust(
    app: AppHandle,
    paths: Vec<String>,
    brightness: i32,
    contrast: i32,
    saturation: i32,
    filter: String,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        run_batch(&app, paths, move |src| {
            let dst = adjust_file(src, brightness, contrast, saturation, &filter)?;
            Ok((
                dst,
                Some(match filter.as_str() {
                    "gray" => Note::new("note.grayscaled"),
                    "sepia" => Note::new("note.sepia"),
                    "invert" => Note::new("note.inverted"),
                    _ => Note::new("note.adjusted")
                        .with("b", brightness)
                        .with("c", contrast)
                        .with("s", saturation),
                }),
            ))
        })
    })
    .await
    .unwrap_or_default()
}
