use crate::batch::{run_batch, FileOutcome, Note};
use crate::err::{AppError, AppResult};
use crate::paths::{file_name_of, long_path, output_dir_for, stem_of, unique_path};
use image::{DynamicImage, GenericImageView};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

// ============================================================ 输出格式

#[derive(Clone, Copy, PartialEq)]
pub enum OutFmt {
    Jpeg,
    Png,
    WebP,
    /// 保持原格式
    Keep,
}

impl OutFmt {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => Self::Jpeg,
            "png" => Self::Png,
            "webp" => Self::WebP,
            _ => Self::Keep,
        }
    }

    /// Keep 需要根据源文件扩展名落到一个具体格式上
    pub fn resolve(self, src: &Path) -> Self {
        if self != Self::Keep {
            return self;
        }
        match src
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
            .as_str()
        {
            "png" => Self::Png,
            "webp" => Self::WebP,
            _ => Self::Jpeg,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Jpeg | Self::Keep => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
        }
    }

    /// PNG 是无损的，调「质量」对它没意义——这决定了压到指定体积时能不能靠降质量
    fn is_lossy(self) -> bool {
        matches!(self, Self::Jpeg | Self::WebP)
    }
}

// ============================================================ 编码

/// 这张图有没有真正用到透明通道？
///
/// 只看「是不是 RGBA」不够——大量 PNG 带 alpha 通道但全是不透明的。
/// 要判断转 JPEG 会不会丢东西，得看有没有像素真的不是全不透明。
pub fn has_transparency(img: &DynamicImage) -> bool {
    match img {
        DynamicImage::ImageRgba8(_) | DynamicImage::ImageLumaA8(_) => {
            img.to_rgba8().pixels().any(|p| p.0[3] < 250)
        }
        _ => false,
    }
}

/// 把带透明的图合成到白底再转 RGB。
///
/// JPEG 无法承载 alpha，直接丢弃会让透明区变成纯黑。合成到白底是
/// 大多数工具的默认行为，也是用户看到结果时最不会觉得意外的那个。
fn flatten_on_white(img: &DynamicImage) -> image::RgbImage {
    if !has_transparency(img) {
        return img.to_rgb8();
    }
    let src = img.to_rgba8();
    let mut out = image::RgbImage::new(src.width(), src.height());
    for (x, y, p) in src.enumerate_pixels() {
        let a = p.0[3] as f32 / 255.0;
        let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        out.put_pixel(x, y, image::Rgb([blend(p.0[0]), blend(p.0[1]), blend(p.0[2])]));
    }
    out
}

/// 单独暴露 JPEG 编码，PDF 压缩要用它重压内嵌图片
pub fn encode_jpeg(img: &DynamicImage, quality: u8) -> AppResult<Vec<u8>> {
    encode(img, OutFmt::Jpeg, quality)
}

pub fn encode(img: &DynamicImage, fmt: OutFmt, quality: u8) -> AppResult<Vec<u8>> {
    match fmt {
        OutFmt::Jpeg | OutFmt::Keep => {
            // JPEG 没有透明通道。直接 to_rgb8() 会把完全透明的像素
            // 变成纯黑——一张去了底的 logo 转出来就是个黑块，而软件
            // 一声不吭。先合成到白底，这也是大多数工具的默认行为。
            let rgb = flatten_on_white(img);
            let (w, h) = rgb.dimensions();
            // mozjpeg 在同等画质下比标准编码器小 10~20%，是压缩效果的关键
            let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
            comp.set_size(w as usize, h as usize);
            comp.set_quality(quality as f32);
            let mut started = comp
                .start_compress(Vec::new())
                .map_err(|e| AppError::unknown(e))?;
            started
                .write_scanlines(rgb.as_raw())
                .map_err(|e| AppError::unknown(e))?;
            started.finish().map_err(|e| AppError::unknown(e))
        }
        OutFmt::WebP => {
            let encoder =
                webp::Encoder::from_image(img).map_err(|e| AppError::unknown(e.to_string()))?;
            Ok(encoder.encode(quality as f32).to_vec())
        }
        OutFmt::Png => {
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png)
                .map_err(|e| AppError::unknown(e))?;
            // PNG 无损，靠 oxipng 再榨一轮体积
            let raw = buf.into_inner();
            let opts = oxipng::Options::from_preset(2);
            Ok(oxipng::optimize_from_memory(&raw, &opts).unwrap_or(raw))
        }
    }
}

// ============================================ 差异化功能：压到指定体积

const MIN_Q: u8 = 20;
const MAX_Q: u8 = 95;
/// 每轮缩放的比例。降到最低画质仍超标时，只能动分辨率了。
const SHRINK: f32 = 0.8;
const MAX_SHRINK_ROUNDS: u32 = 6;
/// 质量二分的收敛容差。差 3 档画质肉眼分不出来，
/// 但少迭代两轮能省掉两次整图编码——大图上这是几秒的差别。
const Q_TOLERANCE: u8 = 3;
/// 从最高质量降到最低质量，体积大致能再降到 1/4。
/// 超过这个倍数就只能动分辨率，据此一步估算出初始缩放系数，
/// 避免从 100% 开始一轮轮试——那是实测里 44 秒的元凶。
const QUALITY_HEADROOM: f32 = 4.0;

pub struct TargetResult {
    pub bytes: Vec<u8>,
    pub quality: u8,
    /// 相对原图的尺寸百分比，100 表示没缩放
    pub scale_pct: u32,
    /// 尽了最大努力仍超出目标
    pub overshoot: bool,
}

/// 二分搜索质量参数逼近目标体积。
///
/// 这是竞品做不到的点：TinyPNG 之类只能让你选一个质量档位，
/// 没法保证「每张都在 500KB 以内」。而各种网站的上传限制正是按体积卡的。
pub fn compress_to_target(
    img: &DynamicImage,
    fmt: OutFmt,
    target: usize,
) -> AppResult<TargetResult> {
    // PNG 无损，没有质量维度可调，只能靠缩放
    if !fmt.is_lossy() {
        return shrink_until_fits(img, fmt, target, MAX_Q);
    }

    let (ow, oh) = img.dimensions();

    // 第一步：原尺寸最高质量试一次，作为体积基准
    let best_case = encode(img, fmt, MAX_Q)?;
    if best_case.len() <= target {
        return Ok(TargetResult {
            bytes: best_case,
            quality: MAX_Q,
            scale_pct: 100,
            overshoot: false,
        });
    }

    // 第二步：一步估算初始缩放，而不是一轮轮 ×0.8 地试。
    // 体积近似正比于像素数，所以缩放系数取面积比的平方根。
    let over = best_case.len() as f32 / target as f32;
    let mut work = if over > QUALITY_HEADROOM {
        let scale = (QUALITY_HEADROOM / over).sqrt().clamp(0.05, 1.0);
        img.resize(
            ((ow as f32 * scale) as u32).max(16),
            ((oh as f32 * scale) as u32).max(16),
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img.clone()
    };

    for round in 0..=MAX_SHRINK_ROUNDS {
        // 二分：找满足 size <= target 的最大质量，带容差提前收敛
        let (mut lo, mut hi) = (MIN_Q, MAX_Q);
        let mut best: Option<(Vec<u8>, u8)> = None;
        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            let data = encode(&work, fmt, mid)?;
            if data.len() <= target {
                let converged = hi.saturating_sub(lo) <= Q_TOLERANCE;
                best = Some((data, mid));
                if converged {
                    break;
                }
                lo = mid + 1;
            } else {
                if mid <= MIN_Q {
                    break;
                }
                hi = mid - 1;
            }
        }

        if let Some((bytes, quality)) = best {
            let (cw, _) = work.dimensions();
            return Ok(TargetResult {
                bytes,
                quality,
                scale_pct: (cw * 100 / ow.max(1)).max(1),
                overshoot: false,
            });
        }

        // 估算偏乐观：最低质量仍超标，继续缩
        if round == MAX_SHRINK_ROUNDS {
            break;
        }
        let (w, h) = work.dimensions();
        work = work.resize(
            ((w as f32 * SHRINK) as u32).max(16),
            ((h as f32 * SHRINK) as u32).max(16),
            image::imageops::FilterType::Lanczos3,
        );
    }

    // 尽力了仍超标——如实返回并标记，不假装成功
    let bytes = encode(&work, fmt, MIN_Q)?;
    let (cw, _) = work.dimensions();
    Ok(TargetResult {
        bytes,
        quality: MIN_Q,
        scale_pct: (cw * 100 / ow.max(1)).max(1),
        overshoot: true,
    })
}

fn shrink_until_fits(
    img: &DynamicImage,
    fmt: OutFmt,
    target: usize,
    quality: u8,
) -> AppResult<TargetResult> {
    let mut work = img.clone();
    let (ow, _) = img.dimensions();
    for round in 0..=MAX_SHRINK_ROUNDS {
        let data = encode(&work, fmt, quality)?;
        let (cw, _) = work.dimensions();
        if data.len() <= target || round == MAX_SHRINK_ROUNDS {
            return Ok(TargetResult {
                bytes: data,
                quality,
                scale_pct: (cw * 100 / ow.max(1)).max(1),
                overshoot: false,
            });
        }
        let (w, h) = work.dimensions();
        work = work.resize(
            ((w as f32 * SHRINK) as u32).max(16),
            ((h as f32 * SHRINK) as u32).max(16),
            image::imageops::FilterType::Lanczos3,
        );
    }
    unreachable!()
}

// ============================================================ 批量运行器
// FileOutcome / Progress / run_batch 已提取到 crate::batch，
// 供图片、PDF、OCR 等所有支柱共用，前端因此只需要认一种结果结构。

pub fn load(src: &Path) -> AppResult<DynamicImage> {
    let bytes = std::fs::read(long_path(src))?;
    image::load_from_memory(&bytes).map_err(|e| {
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "图片".into());
        AppError::decode(&ext, e)
    })
}

/// 文件加入列表时的元信息。
///
/// 没有它的话列表里只能显示「0 B」，用户在点「开始」之前
/// 完全不知道自己选了多大的东西——这在批量工具里是硬伤。
#[derive(Serialize)]
pub struct FileMeta {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub exists: bool,
}

/// 一次拖进来的东西里，直接展开成能处理的文件清单。
///
/// 「批量压缩」的说明写着「一次压一整个文件夹」，可拖文件夹进来什么都不会发生
/// ——扩展名过滤把目录本身筛掉了。文案承诺了的事就得做得到。
///
/// 两条规矩：
///  · 跳过我们自己的输出目录。不然跑第二遍会把上一遍的产物再压一次，
///    越压越糊，而用户完全不知道发生了什么。
///  · 有上限。误拖一个 C:\ 进来不该让程序卡死在遍历上。
#[tauri::command]
pub async fn expand_inputs(paths: Vec<String>, accepts: Vec<String>) -> Vec<String> {
    const MAX: usize = 20_000;
    tauri::async_runtime::spawn_blocking(move || {
        let matches = |p: &Path| -> bool {
            if accepts.is_empty() {
                return true;
            }
            p.extension()
                .map(|e| accepts.contains(&e.to_string_lossy().to_lowercase()))
                .unwrap_or(false)
        };

        let mut out = Vec::new();
        for raw in paths {
            let pb = PathBuf::from(&raw);
            if !long_path(&pb).is_dir() {
                if matches(&pb) {
                    out.push(raw);
                }
                continue;
            }
            for entry in jwalk::WalkDir::new(long_path(&pb))
                .skip_hidden(false)
                .into_iter()
                .flatten()
            {
                if out.len() >= MAX {
                    break;
                }
                let p = entry.path();
                if !entry.file_type().is_file() || !matches(&p) {
                    continue;
                }
                // 别把上一轮的产物当输入再跑一遍
                if p.components().any(|c| c.as_os_str() == crate::paths::OUTPUT_DIR) {
                    continue;
                }
                out.push(p.to_string_lossy().to_string());
            }
        }
        out.sort();
        out
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn stat_files(paths: Vec<String>) -> Vec<FileMeta> {
    paths
        .into_iter()
        .map(|p| {
            let pb = PathBuf::from(&p);
            let meta = std::fs::metadata(long_path(&pb));
            FileMeta {
                name: file_name_of(&pb),
                bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                exists: meta.is_ok(),
                path: p,
            }
        })
        .collect()
}

/// 一张缩略图，直接嵌成 data URI 给界面用。
///
/// 走命令通道而不是开 Tauri 的 asset 协议：开协议要放宽 CSP、给出可读目录范围，
/// 为了几个 64 像素的方块扩这么大一片攻击面不值得。而且在 Rust 这边缩放，
/// 界面拿到的是几 KB，不是原图那 4.6 MB。
#[derive(Serialize)]
pub struct Thumb {
    pub path: String,
    /// 生成失败（非图片、损坏、无权限）就是 None，界面回落到空位图
    pub data_url: Option<String>,
}

const THUMB_PX: u32 = 96;

fn thumb_of(src: &Path) -> Option<String> {
    let img = image::open(long_path(src)).ok()?;
    let small = img.thumbnail(THUMB_PX, THUMB_PX).to_rgb8();
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 70)
        .encode(&small, small.width(), small.height(), image::ExtendedColorType::Rgb8)
        .ok()?;
    use base64::Engine;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&buf)
    ))
}

/// 缩略图是「看得见自己选了什么」的关键 —— 一列文件名分不清哪张是哪张。
/// 前端异步调用，慢也不挡主流程。
#[tauri::command]
pub async fn thumbs(paths: Vec<String>) -> Vec<Thumb> {
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| Thumb {
                data_url: thumb_of(Path::new(&p)),
                path: p,
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

pub fn write_out(src: &Path, fmt: OutFmt, data: &[u8]) -> AppResult<PathBuf> {
    let dir = output_dir_for(src)?;
    let dst = unique_path(&dir, &stem_of(src), fmt.ext());
    std::fs::write(long_path(&dst), data)?;
    Ok(dst)
}

// ============================================================ 命令

fn img_compress_target_blocking(
    app: AppHandle,
    paths: Vec<String>,
    target_kb: u32,
    format: String,
) -> Vec<FileOutcome> {
    let target = (target_kb as usize) * 1024;
    let want = OutFmt::parse(&format);
    run_batch(&app, paths, |src| {
        let src_len = std::fs::metadata(long_path(src)).map(|m| m.len()).unwrap_or(0) as usize;

        // 原图本来就达标，且换格式只会更大——直接原样复制。
        // 用户点了「压缩」结果文件变大，是说不过去的。
        if src_len > 0 && src_len <= target {
            let img = load(src)?;
            let fmt = want.resolve(src);
            let r = compress_to_target(&img, fmt, target)?;
            if r.bytes.len() >= src_len {
                let dir = output_dir_for(src)?;
                let ext = src
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_else(|| "jpg".into());
                let dst = unique_path(&dir, &stem_of(src), &ext);
                std::fs::copy(long_path(src), long_path(&dst))?;
                return Ok((dst, Some(Note::new("note.alreadyUnderTarget"))));
            }
            let dst = write_out(src, fmt, &r.bytes)?;
            return Ok((dst, Some(Note::new("note.quality").with("q", r.quality))));
        }

        let img = load(src)?;
        let fmt = want.resolve(src);
        let lost_alpha = fmt == OutFmt::Jpeg && has_transparency(&img);
        let r = compress_to_target(&img, fmt, target)?;
        let dst = write_out(src, fmt, &r.bytes)?;
        let mut note = Note::new("note.quality").with("q", r.quality);
        if lost_alpha {
            note = note.plus("note.alphaFlattened");
        }
        if r.scale_pct < 100 {
            note = note.plus("note.scaled").with("pct", r.scale_pct);
        }
        if r.overshoot {
            note = note.plus("note.overshoot");
        }
        Ok((dst, Some(note)))
    })
}

fn img_compress_blocking(
    app: AppHandle,
    paths: Vec<String>,
    quality: u8,
) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let img = load(src)?;
        let fmt = OutFmt::Keep.resolve(src);
        let data = encode(&img, fmt, quality)?;
        let dst = write_out(src, fmt, &data)?;
        Ok((dst, Some(Note::new("note.quality").with("q", quality))))
    })
}

fn img_convert_blocking(app: AppHandle, paths: Vec<String>, format: String) -> Vec<FileOutcome> {
    let fmt = OutFmt::parse(&format);
    run_batch(&app, paths, move |src| {
        let img = load(src)?;
        let f = fmt.resolve(src);
        let lost_alpha = f == OutFmt::Jpeg && has_transparency(&img);
        let data = encode(&img, f, 90)?;
        let dst = write_out(src, f, &data)?;
        let mut note = Note::new("note.converted").with("fmt", f.ext().to_uppercase());
        if lost_alpha {
            note = note.plus("note.alphaFlattened");
        }
        Ok((dst, Some(note)))
    })
}

fn img_resize_blocking(app: AppHandle, paths: Vec<String>, long_edge: u32) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let img = load(src)?;
        let (w, h) = img.dimensions();
        // 只缩不放：把小图放大只会变糊且变大，不是用户想要的
        if w.max(h) <= long_edge {
            let fmt = OutFmt::Keep.resolve(src);
            let data = encode(&img, fmt, 92)?;
            let dst = write_out(src, fmt, &data)?;
            return Ok((
                dst,
                Some(Note::new("note.resizeSkipped").with("w", w).with("h", h)),
            ));
        }
        let scaled = img.resize(long_edge, long_edge, image::imageops::FilterType::Lanczos3);
        let (nw, nh) = scaled.dimensions();
        let fmt = OutFmt::Keep.resolve(src);
        let data = encode(&scaled, fmt, 92)?;
        let dst = write_out(src, fmt, &data)?;
        Ok((
            dst,
            Some(
                Note::new("note.resized")
                    .with("w", w)
                    .with("h", h)
                    .with("nw", nw)
                    .with("nh", nh),
            ),
        ))
    })
}

// ------------------------------------- 差异化功能：抹除 EXIF 隐私信息

/// 检查是否含 GPS 定位，用于给用户一个具体的提醒
fn has_gps(bytes: &[u8]) -> bool {
    let mut cur = std::io::Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cur) else {
        return false;
    };
    exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).is_some()
        || exif.get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY).is_some()
}

/// 读 EXIF 里的方向标记（1–8）。没有则返回 1（正常）。
///
/// 这个标记是「相机怎么摆的」——竖着拍的照片，像素常是横着存的，
/// 全靠这个标记告诉看图软件转多少度。抹掉 EXIF 就把它一起抹了，
/// 那些照片就会歪着显示。
pub fn exif_orientation(bytes: &[u8]) -> u32 {
    let mut cur = std::io::Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cur) else {
        return 1;
    };
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1)
}

/// 按方向标记把像素真正转正，转完就不再依赖任何标记。
/// 数值含义是 EXIF 规范定死的 8 种朝向。
pub fn apply_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

fn img_strip_exif_blocking(
    app: AppHandle,
    paths: Vec<String>,
    keep_orientation: bool,
) -> Vec<FileOutcome> {
    run_batch(&app, paths, move |src| {
        let bytes = std::fs::read(long_path(src))?;
        let had_gps = has_gps(&bytes);
        let orientation = exif_orientation(&bytes);

        // 只有「要保方向」且这张确实带了非默认方向标记时，才走重编码那条路。
        // 绝大多数照片方向是 1（正常），照旧走无损剥离，画质不受影响。
        let rotated = keep_orientation && orientation != 1;

        let dst = if rotated {
            // 把像素转正，再编码。转正后不再需要方向标记，抹掉也不会歪。
            // 这条路会重编码，是「不歪」对「无损」的取舍——所以只在必要时走。
            let img = load(src)?;
            let fixed = apply_orientation(img, orientation);
            let fmt = OutFmt::Keep.resolve(src);
            // 质量给高一点，重编码的损失几乎看不出来
            let data = encode(&fixed, fmt, 95)?;
            write_out(src, fmt, &data)?
        } else {
            // 无损路径：img-parts 直接剥掉 EXIF 段，一个像素都不动。
            let mut dyn_img = img_parts::DynImage::from_bytes(bytes.clone().into())
                .map_err(|e| AppError::decode("图片", e))?
                .ok_or_else(|| AppError::new("err.decode").var("format", "图片"))?;
            img_parts::ImageEXIF::set_exif(&mut dyn_img, None);

            let mut out = Vec::with_capacity(bytes.len());
            dyn_img
                .encoder()
                .write_to(&mut out)
                .map_err(|e| AppError::unknown(e))?;

            let dir = output_dir_for(src)?;
            let ext = src
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| "jpg".into());
            let dst = unique_path(&dir, &stem_of(src), &ext);
            std::fs::write(long_path(&dst), &out)?;
            dst
        };

        // 说明要如实：转正过就讲一声（因为重编码了），没转正照旧报 GPS 情况
        let note = if rotated {
            Note::new("note.exifRotated")
        } else if had_gps {
            Note::new("note.exifGps")
        } else {
            Note::new("note.exifNoGps")
        };
        Ok((dst, Some(note)))
    })
}

// ==================================================== 异步命令入口
//
// Tauri 的同步命令在主线程上执行，长任务会冻住整个窗口——
// 界面停在「处理中…」，连进度事件都渲染不出来。所有耗时命令
// 都必须是 async 并把实际工作交给 spawn_blocking。

#[tauri::command]
pub async fn img_compress_target(
    app: AppHandle,
    paths: Vec<String>,
    target_kb: u32,
    format: String,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || {
        img_compress_target_blocking(app, paths, target_kb, format)
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn img_compress(app: AppHandle, paths: Vec<String>, quality: u8) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || img_compress_blocking(app, paths, quality))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn img_convert(app: AppHandle, paths: Vec<String>, format: String) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || img_convert_blocking(app, paths, format))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn img_resize(app: AppHandle, paths: Vec<String>, long_edge: u32) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || img_resize_blocking(app, paths, long_edge))
        .await
        .unwrap_or_default()
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn img_strip_exif(
    app: AppHandle,
    paths: Vec<String>,
    keepOrientation: bool,
) -> Vec<FileOutcome> {
    tauri::async_runtime::spawn_blocking(move || img_strip_exif_blocking(app, paths, keepOrientation))
        .await
        .unwrap_or_default()
}
