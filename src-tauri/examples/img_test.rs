//! 阶段 1 验收：压到指定体积是否真的达标，以及原图是否被动过。
//!
//! 方案里写死的验收标准：
//!   · 「压到 ≤500KB」实测达标率，验证二分搜索确实收敛
//!   · 比对处理前后原文件哈希，证明原图未被修改

use baobox_lib::image_ops::{compress_to_target, OutFmt};
use std::path::Path;
use std::time::Instant;

fn sha_of(p: &Path) -> u64 {
    // 只是用来判断「有没有被改动」，不需要密码学强度
    let data = std::fs::read(p).unwrap();
    let mut h: u64 = 1469598103934665603;
    for b in &data {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"C:\baobox-samples\imgtest".into());
    let targets_kb = [500u32, 200, 100];

    println!("======== 压到指定体积 · 实测 ========\n");

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("测试目录不存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "jpg").unwrap_or(false))
        .collect();
    files.sort();

    let mut pass = 0usize;
    let mut total = 0usize;

    for target_kb in targets_kb {
        let target = target_kb as usize * 1024;
        println!("── 目标：每张 ≤ {target_kb} KB ──");
        for f in &files {
            let before_hash = sha_of(f);
            let before_len = std::fs::metadata(f).unwrap().len();

            let img = match image::open(f) {
                Ok(i) => i,
                Err(e) => {
                    println!(
                        "  {:<16} 解码失败 {e}",
                        f.file_name().unwrap().to_string_lossy()
                    );
                    continue;
                }
            };

            let t0 = Instant::now();
            let r = compress_to_target(&img, OutFmt::WebP, target).expect("压缩失败");
            let ms = t0.elapsed().as_millis();

            total += 1;
            let ok = r.bytes.len() <= target;
            if ok {
                pass += 1;
            }

            // 原图必须原封不动
            let unchanged = sha_of(f) == before_hash;

            println!(
                "  {:<16} {:>7} KB → {:>6} KB  质量{:>3} 缩放{:>3}%  {:>5}ms  {}  原图{}",
                f.file_name().unwrap().to_string_lossy(),
                before_len / 1024,
                r.bytes.len() / 1024,
                r.quality,
                r.scale_pct,
                ms,
                if ok { "达标" } else { "超标" },
                if unchanged { "未动" } else { "!!被改!!" },
            );
        }
        println!();
    }

    println!("======== 达标率 {pass}/{total} ========");
}
