use crate::batch::{run_batch, FileOutcome};
use crate::err::{AppError, AppResult};
use crate::paths::{long_path, output_dir_for, stem_of, unique_path};
use image::GenericImageView;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 图片打码
///
/// 截图发出去之前遮掉身份证号、手机号、住址这类东西。
///
/// **必须真正销毁像素，不能只在上面盖一层。** 网页上那种「加个黑色矩形」
/// 的做法，原始像素还在文件里，随手就能扒出来——那不是打码，是自欺。
/// 这里直接改写像素本身，改完原图无法还原。
/// 这条和 PDF 涂黑密文守的是同一个原则。

#[derive(Deserialize, Clone, Copy)]
pub struct Region {
    /// 相对图片宽高的比例（0–1），这样同一组选区能套用到不同尺寸的图上
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RedactMode {
    Pixelate,
    Blackout,
}

/// 在一张图上抹掉若干区域，返回实际处理的区域数
pub fn redact_image(
    img: &mut image::RgbaImage,
    regions: &[Region],
    mode: RedactMode,
) -> usize {
    let (iw, ih) = (img.width(), img.height());
    let mut done = 0;

    for r in regions {
        let x0 = (r.x.clamp(0.0, 1.0) * iw as f32) as u32;
        let y0 = (r.y.clamp(0.0, 1.0) * ih as f32) as u32;
        let w = ((r.w.max(0.0) * iw as f32) as u32).min(iw.saturating_sub(x0));
        let h = ((r.h.max(0.0) * ih as f32) as u32).min(ih.saturating_sub(y0));
        if w == 0 || h == 0 {
            continue;
        }
        done += 1;

        match mode {
            RedactMode::Blackout => {
                for y in y0..y0 + h {
                    for x in x0..x0 + w {
                        img.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                    }
                }
            }
            RedactMode::Pixelate => {
                // 马赛克块要够大。块太小的话，细节仍然可辨——
                // 手机号被打成勉强能认出的样子，等于没打。
                let block = (w.min(h) / 8).clamp(12, 64);
                let mut by = y0;
                while by < y0 + h {
                    let mut bx = x0;
                    while bx < x0 + w {
                        let bw = block.min(x0 + w - bx);
                        let bh = block.min(y0 + h - by);
                        let (mut r_, mut g_, mut b_, mut n) = (0u32, 0u32, 0u32, 0u32);
                        for y in by..by + bh {
                            for x in bx..bx + bw {
                                let p = img.get_pixel(x, y).0;
                                r_ += p[0] as u32;
                                g_ += p[1] as u32;
                                b_ += p[2] as u32;
                                n += 1;
                            }
                        }
                        let n = n.max(1);
                        let avg = image::Rgba([
                            (r_ / n) as u8,
                            (g_ / n) as u8,
                            (b_ / n) as u8,
                            255,
                        ]);
                        for y in by..by + bh {
                            for x in bx..bx + bw {
                                img.put_pixel(x, y, avg);
                            }
                        }
                        bx += block;
                    }
                    by += block;
                }
            }
        }
    }
    done
}

pub fn redact_file(
    src: &Path,
    regions: &[Region],
    mode: RedactMode,
) -> AppResult<(PathBuf, usize)> {
    let bytes = std::fs::read(long_path(src))?;
    let mut img = image::load_from_memory(&bytes)
        .map_err(|e| AppError::decode("图片", e))?
        .to_rgba8();
    let n = redact_image(&mut img, regions, mode);

    // 一律输出 PNG。若沿用 JPEG，有损压缩会在被遮区域边缘留下
    // 可分析的痕迹；而且重新编码整张图也没必要。
    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &format!("{} 已打码", stem_of(src)), "png");
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AppError::unknown(e))?;
    std::fs::write(long_path(&dst), out.into_inner())?;
    Ok((dst, n))
}

fn img_redact_blocking(
    app: AppHandle,
    paths: Vec<String>,
    regions: Vec<Region>,
    mode: String,
) -> Vec<FileOutcome> {
    let m = if mode == "blackout" {
        RedactMode::Blackout
    } else {
        RedactMode::Pixelate
    };
    run_batch(&app, paths, move |src| {
        let (dst, n) = redact_file(src, &regions, m)?;
        Ok((dst, Some(format!("已抹除 {n} 处"))))
    })
}

#[tauri::command]
pub async fn img_redact(
    app: AppHandle,
    paths: Vec<String>,
    regions: Vec<Region>,
    mode: String,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || img_redact_blocking(app, paths, regions, mode))
        .await
        .unwrap_or_default()
}

/// 读一张图返回 data URL 供前端画选区，同时给出原始尺寸
#[derive(serde::Serialize)]
pub struct ImagePreview {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

#[tauri::command]
pub async fn image_preview(path: String) -> Result<ImagePreview, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = PathBuf::from(&path);
        let bytes = std::fs::read(long_path(&p))?;
        let img = image::load_from_memory(&bytes).map_err(|e| AppError::decode("图片", e))?;
        let (w, h) = img.dimensions();
        // 预览缩到长边 1400 以内，避免几十 MB 的 base64 拖垮界面
        let preview = if w.max(h) > 1400 {
            img.resize(1400, 1400, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        let mut buf = std::io::Cursor::new(Vec::new());
        preview
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .map_err(|e| AppError::unknown(e))?;
        Ok(ImagePreview {
            data_url: format!(
                "data:image/jpeg;base64,{}",
                crate::screen_ocr::b64(&buf.into_inner())
            ),
            width: w,
            height: h,
        })
    })
    .await
    .map_err(|e| AppError::unknown(e))?
}
