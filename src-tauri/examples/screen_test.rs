//! 抓屏验收
//!
//! 抓屏最容易错在两个地方：BGRA 当成 RGBA（红蓝颠倒）和位图上下翻转。
//! 两者都不会报错，只会悄悄给出一张看着「有点怪」的图。所以这里把图
//! 存下来肉眼可查，同时用 OCR 从中读字——能读出屏幕上的文字，就说明
//! 方向和颜色都是对的。

fn main() {
    let out = std::env::temp_dir().join("baobox_screen_test.png");
    println!("======== 抓屏验收 ========\n");

    let t0 = std::time::Instant::now();
    let shot = match tauri::async_runtime::block_on(baobox_lib::screen_ocr::capture_screen()) {
        Ok(s) => s,
        Err(e) => {
            println!("  抓屏失败: {}", e.key);
            return;
        }
    };
    let ms = t0.elapsed().as_millis();
    println!(
        "  [1] 抓取虚拟桌面   {}x{} 起点({},{})  {} ms",
        shot.width, shot.height, shot.origin_x, shot.origin_y, ms
    );

    // data URL 还原成文件，方便肉眼确认颜色和方向
    let b64 = shot.data_url.split(',').nth(1).unwrap_or("");
    let bytes = decode_b64(b64);
    std::fs::write(&out, &bytes).unwrap();
    println!(
        "  [2] PNG 体积       {:.0} KB → {}",
        bytes.len() as f64 / 1024.0,
        out.display()
    );

    // 尺寸得和系统报告的虚拟桌面一致
    let img = image::load_from_memory(&bytes).expect("PNG 解不开");
    let dims_ok = img.width() == shot.width && img.height() == shot.height;
    println!("  [3] 尺寸一致       {}", dims_ok);

    // OCR 整屏。能认出字就说明颜色通道和上下方向都没搞反。
    match baobox_lib::ocr::recognize_for_test(&out) {
        Ok(text) => {
            let chars = text.chars().filter(|c| !c.is_whitespace()).count();
            println!("  [4] OCR 读回       {chars} 个字符");
            let sample: String = text.lines().take(3).collect::<Vec<_>>().join(" / ");
            println!(
                "      样例: {}",
                sample.chars().take(90).collect::<String>()
            );
            println!(
                "\n======== {} ========",
                if chars > 10 && dims_ok {
                    "通过：抓屏方向与颜色正确，屏幕文字可被识别"
                } else {
                    "存疑：没读到足够文字，请打开图片肉眼确认"
                }
            );
        }
        Err(e) => println!("  [4] OCR 失败       {}", e.key),
    }
}

fn decode_b64(s: &str) -> Vec<u8> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let idx = |c: u8| T.iter().position(|&t| t == c).unwrap_or(0) as u32;
    let clean: Vec<u8> = s
        .bytes()
        .filter(|c| *c != b'=' && !c.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= idx(*c) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8)
        }
        if chunk.len() > 3 {
            out.push(n as u8)
        }
    }
    out
}
