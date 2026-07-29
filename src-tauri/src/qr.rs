//! 二维码：生成与识别。
//!
//! 生成和识别是两件完全不同的事，各用各的库——qrcode 只管编码，
//! rqrr 负责在图里找定位图案再解。都是纯 Rust，不引入 C 依赖。

use crate::batch::{run_batch, FileOutcome, Note};
use crate::err::{AppError, AppResult};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use image::Luma;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

// ================================================================ 生成

/// 每行一条内容，批量出图。
///
/// 常见场景是把一列网址、一批设备编号做成二维码贴上去，
/// 手动一个个在网站上生成再下载是纯粹的重复劳动。
pub fn generate_from_file(src: &Path, px: u32) -> AppResult<(PathBuf, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    // 文本文件的编码同样可能是 GBK，复用乱码修复那套检测
    let text = crate::textfile::decode_text(&bytes);

    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(AppError::new("err.emptyFile"));
    }

    let dir = output_dir_for(src)?;
    let stem = stem_of(src);
    let mut last = dir.clone();

    for (i, line) in lines.iter().enumerate() {
        let code = qrcode::QrCode::new(line.as_bytes())
            .map_err(|e| AppError::new("err.qrTooLong").detail(e))?;
        let img = render(&code, px);
        let dst = unique_path(&dir, &format!("{stem}_{:03}", i + 1), "png");
        img.save(long_path(&dst))
            .map_err(|e| AppError::unknown(e))?;
        last = dst;
    }
    Ok((last, lines.len()))
}

/// 自己画位图，不走 qrcode 的渲染器。
///
/// 这样能保证每个模块是整数个像素——按目标尺寸做分数缩放会让模块边缘
/// 出现灰边，扫码器对这个是敏感的，打印出来尤其容易扫不出。
/// 宁可最终尺寸跟请求的差几像素，也要让边缘是硬的。
fn render(code: &qrcode::QrCode, want_px: u32) -> image::GrayImage {
    const QUIET: u32 = 4; // 规范要求的静区，四个模块宽
    let modules = code.width() as u32;
    let total = modules + QUIET * 2;
    let scale = (want_px / total).max(1);
    let side = total * scale;

    let colors = code.to_colors();
    let mut img = image::GrayImage::from_pixel(side, side, Luma([255]));
    for y in 0..modules {
        for x in 0..modules {
            if colors[(y * modules + x) as usize] != qrcode::Color::Dark {
                continue;
            }
            let px = (x + QUIET) * scale;
            let py = (y + QUIET) * scale;
            for dy in 0..scale {
                for dx in 0..scale {
                    img.put_pixel(px + dx, py + dy, Luma([0]));
                }
            }
        }
    }
    img
}

#[tauri::command]
pub async fn qr_generate(app: AppHandle, paths: Vec<String>, size: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        let px = size.clamp(64, 2048);
        run_batch(&app, paths, move |src| {
            let (last, n) = generate_from_file(src, px)?;
            Ok((last, Some(Note::new("note.qrMade").with("n", n))))
        })
    })
    .await
    .unwrap_or_default()
}

// ================================================================ 识别

/// 从一张图里读出所有二维码。
pub fn decode_image(src: &Path) -> AppResult<Vec<String>> {
    let img = crate::image_ops::load(src)?;
    let mut prepared = rqrr::PreparedImage::prepare(img.to_luma8());
    let grids = prepared.detect_grids();

    let mut out = Vec::new();
    for g in grids {
        // 一张图里可能有几个码，其中一两个模糊解不出来——
        // 解得出的照样给，比整张判失败有用
        if let Ok((_, content)) = g.decode() {
            if !content.is_empty() {
                out.push(content);
            }
        }
    }
    if out.is_empty() {
        return Err(AppError::new("err.qrNotFound"));
    }
    Ok(out)
}

#[tauri::command]
pub async fn qr_decode(app: AppHandle, paths: Vec<String>) -> Vec<FileOutcome> {
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
            crate::batch::emit_working(&app, index, total, &crate::paths::file_name_of(&src));

            let o = match decode_image(&src) {
                Ok(list) => FileOutcome::text_only(
                    &src,
                    list.join("\n"),
                    Some(Note::new("note.qrFound").with("n", list.len())),
                ),
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
