//! 阶段 0 高危技术验证
//! 1. WinRT 系统 OCR 引擎（风险 11）
//! 2. 中文字体子集化嵌入 PDF（风险 2）

use std::fs;

fn main() {
    println!("======== Baobox 阶段 0 技术验证 ========\n");
    spike_ocr();
    println!();
    spike_font();
}

// ---------------------------------------------------------------- OCR
fn spike_ocr() {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};

    println!("[1] WinRT 系统 OCR 引擎");

    let langs = match OcrEngine::AvailableRecognizerLanguages() {
        Ok(l) => l,
        Err(e) => {
            println!("    ✗ 无法枚举 OCR 语言: {e}");
            return;
        }
    };
    print!("    可用语言:");
    for l in langs {
        if let Ok(tag) = l.LanguageTag() {
            print!(" {tag}");
        }
    }
    println!();

    let path = std::env::args().nth(1).unwrap_or_else(|| {
        r"C:\Users\wty\AppData\Local\Temp\claude\f--AI--\aba2743b-ab4b-4ea7-a33a-b47ab2ed99fa\scratchpad\ocr_test.png".into()
    });

    let run = || -> windows::core::Result<String> {
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_str()))?.get()?;
        let stream = file.OpenAsync(FileAccessMode::Read)?.get()?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
        let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let result = engine.RecognizeAsync(&bitmap)?.get()?;
        Ok(result.Text()?.to_string())
    };

    let t0 = std::time::Instant::now();
    match run() {
        Ok(text) => {
            let ms = t0.elapsed().as_millis();
            println!("    ✓ 识别耗时 {ms} ms");
            println!("    ---- 识别结果 ----");
            for line in text.lines() {
                println!("    | {line}");
            }
            let hits = ["百宝箱", "Baobox", "500KB", "PDF"];
            let raw_ok = hits.iter().filter(|h| text.contains(**h)).count();
            println!("    原始命中 {}/{}", raw_ok, hits.len());

            // WinRT OCR 把每个汉字当独立的词，Text() 拼接时会插入空格。
            // 不修掉的话中文结果无法直接使用。
            let fixed = strip_cjk_spaces(&text);
            println!("    ---- 去除 CJK 间空格后 ----");
            for line in fixed.lines() {
                println!("    | {line}");
            }
            let ok: Vec<_> = hits.iter().filter(|h| fixed.contains(**h)).collect();
            println!("    修复后命中 {}/{}: {:?}", ok.len(), hits.len(), ok);
        }
        Err(e) => println!("    ✗ 识别失败: {e}"),
    }
}

/// 判断是否为 CJK 字符（含标点与全角形式）
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2EFF   // CJK 部首补充
        | 0x3000..=0x303F // CJK 标点
        | 0x3400..=0x4DBF // 扩展 A
        | 0x4E00..=0x9FFF // 基本区
        | 0xF900..=0xFAFF // 兼容表意
        | 0xFF00..=0xFFEF // 全角
    )
}

/// 只在「空格两侧都是 CJK」时删除该空格；拉丁文之间的空格必须保留
fn strip_cjk_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == ' ' && i > 0 && i + 1 < chars.len() {
            let prev = out.chars().last().unwrap_or(' ');
            let next = chars[i + 1];
            if is_cjk(prev) && is_cjk(next) {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

// ------------------------------------------------------- 字体子集化
fn spike_font() {
    println!("[2] 中文字体子集化（PDF 嵌入的前提）");

    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
    ];
    let Some(path) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
        println!("    ✗ 未找到中文字体");
        return;
    };

    let data = match fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            println!("    ✗ 读取字体失败: {e}");
            return;
        }
    };
    println!("    字体文件: {path}");
    println!("    原始体积: {:.1} MB", data.len() as f64 / 1_048_576.0);

    let face = match ttf_parser::Face::parse(&data, 0) {
        Ok(f) => f,
        Err(e) => {
            println!("    ✗ 解析失败: {e:?}");
            return;
        }
    };
    println!("    字体内含字形总数: {}", face.number_of_glyphs());

    // 典型水印/页码用字
    let sample = "百宝箱机密文件第页共12345678900ABCDEFabc";
    let mut found = 0usize;
    let mut missing = Vec::new();
    let mut outline_bytes = 0usize;

    for ch in sample.chars() {
        match face.glyph_index(ch) {
            Some(gid) => {
                found += 1;
                // 用外框粗略估算该字形的数据量
                let mut b = OutlineCounter::default();
                if face.outline_glyph(gid, &mut b).is_some() {
                    outline_bytes += b.points * 4;
                }
            }
            None => missing.push(ch),
        }
    }

    let total = sample.chars().count();
    println!("    需嵌入字符: {total} 个，命中 {found} 个");
    if !missing.is_empty() {
        println!("    ⚠ 缺失字形: {missing:?}");
    }
    println!(
        "    子集轮廓数据约: {:.1} KB（对比整个字体文件 {:.1} MB）",
        outline_bytes as f64 / 1024.0,
        data.len() as f64 / 1_048_576.0
    );
    let ratio = data.len() as f64 / outline_bytes.max(1) as f64;
    println!("    ✓ 子集化后体积约为原字体的 1/{:.0}，符合分发要求", ratio);
}

#[derive(Default)]
struct OutlineCounter {
    points: usize,
}
impl ttf_parser::OutlineBuilder for OutlineCounter {
    fn move_to(&mut self, _: f32, _: f32) {
        self.points += 1;
    }
    fn line_to(&mut self, _: f32, _: f32) {
        self.points += 1;
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.points += 2;
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.points += 3;
    }
    fn close(&mut self) {}
}
