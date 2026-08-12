//! 从 PDF 的图片流里解出像素。
//!
//! # 为什么要自己写
//!
//! lopdf 的 `Stream::decompressed_content()` 开头有这么一句：
//!
//! ```text
//! if self.dict.get(b"Subtype").and_then(Object::as_name_str).ok() == Some("Image") {
//!     return Err(Error::Type);
//! }
//! ```
//!
//! 也就是说，**对内嵌图片它一律拒绝**。而 PDF 压缩里那条 FlateDecode 分支正是
//! 靠它来拿裸像素的——那条分支从写下来的第一天起就没跑通过一次，只是失败得
//! 很安静：解压返回 Err，代码 `continue`，图片被跳过，最后报告「重压 N 张」，
//! N 只统计了 JPEG 那一半。
//!
//! 当初的实测数据是「DCTDecode 3453 张 297 MB，FlateDecode 3358 张 407 MB」，
//! 也就是按体积算漏掉的比处理掉的还多。压缩率的账一直是对的（产物确实变小了），
//! 只是本可以更小，而没人会发现。
//!
//! # 预测器
//!
//! PDF 的 Flate 流常带 `/DecodeParms << /Predictor 15 ... >>`，用的是 PNG 那套
//! 逐行滤波。不还原的话解出来是一片斜纹噪声——而且同样不会报错，
//! 只会产出一张看起来「坏了」的图。

use lopdf::{Dictionary, Object, Stream};

/// 把图片流解成裸像素。失败返回 None——上层一律当作「这张跳过」。
pub fn raw_pixels(stream: &Stream) -> Option<Vec<u8>> {
    let filter = single_filter(stream)?;
    if filter != "FlateDecode" {
        return None;
    }

    let inflated = inflate(&stream.content)?;
    let params = decode_parms(stream);
    Some(undo_predictor(inflated, params.as_ref()))
}

/// 只认单一过滤器。串联多个（如 ASCII85 + Flate）的情况少见，
/// 猜错了产出的是花屏，宁可跳过。
pub fn single_filter(stream: &Stream) -> Option<String> {
    match stream.dict.get(b"Filter") {
        Ok(Object::Name(n)) => Some(String::from_utf8_lossy(n).to_string()),
        Ok(Object::Array(a)) if a.len() == 1 => a[0]
            .as_name()
            .ok()
            .map(|n| String::from_utf8_lossy(n).to_string()),
        _ => None,
    }
}

fn decode_parms(stream: &Stream) -> Option<Dictionary> {
    match stream.dict.get(b"DecodeParms") {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        // 数组形式对应串联过滤器，取第一个就够
        Ok(Object::Array(a)) => a.iter().find_map(|o| o.as_dict().ok().cloned()),
        _ => None,
    }
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut out = Vec::with_capacity(data.len() * 3);
    let mut dec = ZlibDecoder::new(data);
    match dec.read_to_end(&mut out) {
        Ok(_) => Some(out),
        // 截断的流也可能已经解出了大部分——有些 PDF 就是这么写的。
        // 拿到多少算多少，长度不够的话上层会挡掉。
        Err(_) if !out.is_empty() => Some(out),
        Err(_) => None,
    }
}

/// 还原 PNG 逐行滤波（Predictor >= 10）。
///
/// Predictor 2 是 TIFF 的差分预测，用得极少，这里不处理——
/// 返回原样比返回一张猜错的图诚实。
fn undo_predictor(data: Vec<u8>, params: Option<&Dictionary>) -> Vec<u8> {
    let Some(p) = params else { return data };
    let get = |k: &[u8], def: i64| p.get(k).and_then(|o| o.as_i64()).unwrap_or(def);

    let predictor = get(b"Predictor", 1);
    if predictor < 10 {
        return data;
    }

    let colors = get(b"Colors", 1).max(1) as usize;
    let bpc = get(b"BitsPerComponent", 8).max(1) as usize;
    let columns = get(b"Columns", 1).max(1) as usize;

    // 每像素字节数，至少 1（小于 8 位时按 1 算，正好符合规范）
    let bpp = (colors * bpc + 7) / 8;
    let row_len = (columns * colors * bpc + 7) / 8;
    if row_len == 0 {
        return data;
    }

    let rows = data.len() / (row_len + 1);
    let mut out = Vec::with_capacity(rows * row_len);
    let mut prev = vec![0u8; row_len];

    for r in 0..rows {
        let base = r * (row_len + 1);
        let ft = data[base];
        let mut row = data[base + 1..base + 1 + row_len].to_vec();

        for i in 0..row_len {
            let a = if i >= bpp { row[i - bpp] } else { 0 }; // 左
            let b = prev[i]; // 上
            let c = if i >= bpp { prev[i - bpp] } else { 0 }; // 左上
            row[i] = match ft {
                0 => row[i],
                1 => row[i].wrapping_add(a),
                2 => row[i].wrapping_add(b),
                3 => row[i].wrapping_add((((a as u16) + (b as u16)) / 2) as u8),
                4 => row[i].wrapping_add(paeth(a, b, c)),
                // 未知滤波类型，原样放过，别把整张图搞成噪声
                _ => row[i],
            };
        }
        out.extend_from_slice(&row);
        prev = row;
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// 把裸像素包成 image 的对象。颜色空间不认识就返回 None——
/// Indexed / ICCBased 要查调色板或色彩配置，猜错了导出来是花的。
pub fn to_image(stream: &Stream, raw: &[u8], w: u32, h: u32) -> Option<image::DynamicImage> {
    let cs = match stream.dict.get(b"ColorSpace") {
        Ok(Object::Name(n)) => String::from_utf8_lossy(n).to_string(),
        _ => return None,
    };
    let bpc = stream
        .dict
        .get(b"BitsPerComponent")
        .and_then(|o| o.as_i64())
        .unwrap_or(8);
    if bpc != 8 {
        return None;
    }

    match cs.as_str() {
        "DeviceRGB" => {
            let need = w as usize * h as usize * 3;
            if raw.len() < need {
                return None;
            }
            image::RgbImage::from_raw(w, h, raw[..need].to_vec()).map(Into::into)
        }
        "DeviceGray" => {
            let need = w as usize * h as usize;
            if raw.len() < need {
                return None;
            }
            image::GrayImage::from_raw(w, h, raw[..need].to_vec()).map(Into::into)
        }
        "DeviceCMYK" => {
            let need = w as usize * h as usize * 4;
            if raw.len() < need {
                return None;
            }
            // PDF 里的 CMYK 是「墨量」，0 表示不上墨即最亮，跟常见的反过来
            let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
            for px in raw[..need].chunks_exact(4) {
                let k = px[3] as u32;
                for i in 0..3 {
                    rgb.push((255 - (px[i] as u32 + k).min(255)) as u8);
                }
            }
            image::RgbImage::from_raw(w, h, rgb).map(Into::into)
        }
        _ => None,
    }
}
