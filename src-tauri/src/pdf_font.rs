use crate::err::{AppError, AppResult};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::BTreeMap;

/// 中文字体子集化嵌入（方案风险 2）
///
/// 给 PDF 加中文水印或页码，字体必须嵌进文件，否则在没装这个字体的
/// 电脑上就是乱码或空白。但微软雅黑本体 18.8 MB，而且**受版权保护，
/// 不允许随安装包分发**。
///
/// 解法是读系统字体、只把实际用到的那几十个字形抽出来嵌入：
/// 体积降到几十 KB，也符合字体自身的嵌入授权。

/// 候选字体，按优先级排列。都是 Windows 自带的。
const CANDIDATES: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\msyh.ttc", 0),   // 微软雅黑
    (r"C:\Windows\Fonts\simhei.ttf", 0), // 黑体
    (r"C:\Windows\Fonts\simsun.ttc", 0), // 宋体
    (r"C:\Windows\Fonts\arial.ttf", 0),  // 兜底：只有拉丁字母时够用
];

pub struct EmbeddedFont {
    /// 子集后的字体二进制
    pub data: Vec<u8>,
    /// 字符 → 子集里的新字形编号
    pub gid_of: BTreeMap<char, u16>,
    /// 新字形编号 → 宽度（已归一化到 1000 单位）
    pub width_of: BTreeMap<u16, u16>,
    pub ascent: i16,
    pub descent: i16,
    pub bbox: [i16; 4],
    /// 原始字体文件体积，用于报告压缩效果
    pub source_bytes: usize,
}

impl EmbeddedFont {
    /// 把一段文字编码成 Identity-H 下的十六进制串（每字形 2 字节）
    pub fn encode(&self, text: &str) -> String {
        let mut s = String::with_capacity(text.chars().count() * 4);
        for ch in text.chars() {
            let gid = self.gid_of.get(&ch).copied().unwrap_or(0);
            s.push_str(&format!("{gid:04X}"));
        }
        s
    }

    /// 估算一段文字在给定字号下的宽度，用于居中排版
    pub fn width_of_text(&self, text: &str, size: f32) -> f32 {
        let units: u32 = text
            .chars()
            .map(|c| {
                self.gid_of
                    .get(&c)
                    .and_then(|g| self.width_of.get(g))
                    .copied()
                    .unwrap_or(500) as u32
            })
            .sum();
        units as f32 * size / 1000.0
    }
}

/// 为一段文字准备好可嵌入的子集字体
pub fn prepare(text: &str) -> AppResult<EmbeddedFont> {
    let mut chars: Vec<char> = text.chars().collect();
    chars.sort_unstable();
    chars.dedup();
    if chars.is_empty() {
        return Err(AppError::new("err.stampEmpty"));
    }

    for (path, index) in CANDIDATES {
        if !std::path::Path::new(path).exists() {
            continue;
        }
        let Ok(data) = std::fs::read(path) else { continue };
        let Ok(face) = ttf_parser::Face::parse(&data, *index) else {
            continue;
        };

        // 这个字体能不能覆盖全部字符？缺字就换下一个，
        // 半数字形缺失的水印比没有水印更糟。
        let mut old_gids = Vec::with_capacity(chars.len());
        let mut missing = false;
        for ch in &chars {
            match face.glyph_index(*ch) {
                Some(g) => old_gids.push((*ch, g.0)),
                None => {
                    missing = true;
                    break;
                }
            }
        }
        if missing {
            continue;
        }

        let mut remapper = subsetter::GlyphRemapper::new();
        let mut gid_of = BTreeMap::new();
        let mut width_of = BTreeMap::new();
        let upem = face.units_per_em().max(1) as f32;

        for (ch, old) in &old_gids {
            let new = remapper.remap(*old);
            gid_of.insert(*ch, new);
            let adv = face
                .glyph_hor_advance(ttf_parser::GlyphId(*old))
                .unwrap_or(500) as f32;
            width_of.insert(new, (adv * 1000.0 / upem).round() as u16);
        }

        let subset = subsetter::subset(&data, *index, &remapper)
            .map_err(|e| AppError::new("err.fontSubset").detail(format!("{e:?}")))?;

        let bb = face.global_bounding_box();
        let scale = 1000.0 / upem;
        return Ok(EmbeddedFont {
            data: subset,
            gid_of,
            width_of,
            ascent: (face.ascender() as f32 * scale) as i16,
            descent: (face.descender() as f32 * scale) as i16,
            bbox: [
                (bb.x_min as f32 * scale) as i16,
                (bb.y_min as f32 * scale) as i16,
                (bb.x_max as f32 * scale) as i16,
                (bb.y_max as f32 * scale) as i16,
            ],
            source_bytes: data.len(),
        });
    }

    Err(AppError::new("err.fontMissing"))
}

/// 把子集字体写进文档，返回 Type0 字体对象 id。
///
/// 用 Identity-H 编码 + CIDFontType2：字符串里直接放字形编号，
/// 绕开各种编码表的坑，中文一次到位。
pub fn embed(doc: &mut Document, font: &EmbeddedFont) -> ObjectId {
    let len1 = font.data.len();
    let mut file_dict = Dictionary::new();
    file_dict.set("Length1", len1 as i64);
    let mut file_stream = Stream::new(file_dict, font.data.clone());
    let _ = file_stream.compress();
    let file_id = doc.add_object(Object::Stream(file_stream));

    let mut descriptor = Dictionary::new();
    descriptor.set("Type", "FontDescriptor");
    descriptor.set("FontName", Object::Name(b"BAOBOX+Subset".to_vec()));
    // 4 = Symbolic，CJK 子集用它最省事
    descriptor.set("Flags", 4i64);
    descriptor.set(
        "FontBBox",
        font.bbox.iter().map(|v| Object::Integer(*v as i64)).collect::<Vec<_>>(),
    );
    descriptor.set("ItalicAngle", 0i64);
    descriptor.set("Ascent", font.ascent as i64);
    descriptor.set("Descent", font.descent as i64);
    descriptor.set("CapHeight", font.ascent as i64);
    descriptor.set("StemV", 80i64);
    descriptor.set("FontFile2", file_id);
    let descriptor_id = doc.add_object(Object::Dictionary(descriptor));

    // W 数组：逐个字形声明宽度，缺的走 DW 默认值
    let mut w = Vec::new();
    for (gid, width) in &font.width_of {
        w.push(Object::Integer(*gid as i64));
        w.push(Object::Array(vec![Object::Integer(*width as i64)]));
    }

    let mut cid_sys = Dictionary::new();
    cid_sys.set("Registry", Object::string_literal("Adobe"));
    cid_sys.set("Ordering", Object::string_literal("Identity"));
    cid_sys.set("Supplement", 0i64);

    let mut descendant = Dictionary::new();
    descendant.set("Type", "Font");
    descendant.set("Subtype", "CIDFontType2");
    descendant.set("BaseFont", Object::Name(b"BAOBOX+Subset".to_vec()));
    descendant.set("CIDSystemInfo", Object::Dictionary(cid_sys));
    descendant.set("FontDescriptor", descriptor_id);
    descendant.set("DW", 1000i64);
    descendant.set("W", Object::Array(w));
    descendant.set("CIDToGIDMap", Object::Name(b"Identity".to_vec()));
    let descendant_id = doc.add_object(Object::Dictionary(descendant));

    let mut font_dict = Dictionary::new();
    font_dict.set("Type", "Font");
    font_dict.set("Subtype", "Type0");
    font_dict.set("BaseFont", Object::Name(b"BAOBOX+Subset".to_vec()));
    font_dict.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    font_dict.set("DescendantFonts", vec![Object::Reference(descendant_id)]);
    doc.add_object(Object::Dictionary(font_dict))
}

/// 半透明用的图形状态。水印必须能透出底下的内容，否则就是涂抹而不是水印。
pub fn add_alpha_state(doc: &mut Document, alpha: f32) -> ObjectId {
    let mut gs = Dictionary::new();
    gs.set("Type", "ExtGState");
    gs.set("ca", Object::Real(alpha));
    gs.set("CA", Object::Real(alpha));
    doc.add_object(Object::Dictionary(gs))
}

/// 把字体和图形状态挂到页面的 /Resources 上。
///
/// 页面可能没有 Resources，也可能是间接引用；两种都得处理，
/// 否则内容流里引用的 /F1 找不到定义，那一页就什么都不显示。
pub fn attach_resources(
    doc: &mut Document,
    page_id: ObjectId,
    font_id: ObjectId,
    gs_id: Option<ObjectId>,
) {
    let existing = doc
        .get_object(page_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Resources").ok().cloned());

    let mut res = match existing {
        Some(Object::Dictionary(d)) => d,
        Some(Object::Reference(r)) => doc
            .get_object(r)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Dictionary::new(),
    };

    let mut fonts = res
        .get(b"Font")
        .ok()
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    fonts.set("BaoboxF", font_id);
    res.set("Font", Object::Dictionary(fonts));

    if let Some(gs) = gs_id {
        let mut states = res
            .get(b"ExtGState")
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default();
        states.set("BaoboxGS", gs);
        res.set("ExtGState", Object::Dictionary(states));
    }

    if let Ok(page) = doc.get_object_mut(page_id) {
        if let Ok(d) = page.as_dict_mut() {
            d.set("Resources", Object::Dictionary(res));
        }
    }
}
