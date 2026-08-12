//! 重复文件查找验收（只读，绝不删任何东西）
//!
//! 两件事要证明：
//!   1. 找出来的确实是重复文件——抽查几组，逐字节比对
//!   2. 三级筛选真的省了功夫——报告各阶段实际处理了多少

use std::time::Instant;

fn fmt(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.2} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", bytes / 1024)
    }
}

fn main() {
    let roots: Vec<String> = {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            vec![r"F:\".into()]
        } else {
            args
        }
    };

    println!("======== 重复文件查找验收 ========");
    println!("扫描根: {roots:?}\n");

    let t0 = Instant::now();
    let last_phase = std::cell::Cell::new("");
    let report = baobox_lib::dedupe::scan(&roots, &|phase, done, total| {
        if phase != last_phase.get() {
            last_phase.set(phase);
            let label = match phase {
                "walk" => "① 遍历分组",
                "quick" => "② 首尾快哈希",
                "full" => "③ 全文件哈希",
                _ => "完成",
            };
            println!("  {label}  起步 {done}/{total}");
        }
    });
    let secs = t0.elapsed().as_secs_f32();

    println!("\n扫描 {} 个文件，耗时 {secs:.1}s", report.scanned);
    println!(
        "跳过云端占位文件 {} 个（读它们会触发下载）",
        report.skipped_cloud
    );
    println!("读取失败 {} 个", report.unreadable);
    println!(
        "找到 {} 组重复，其中 {} 组全部归程序/环境管辖（一份都不建议删）",
        report.groups.len(),
        report.managed_groups
    );
    println!(
        "真正建议删除的可回收量: {}\n",
        fmt(report.total_reclaimable)
    );

    println!("收益最高的前 10 组:");
    for g in report.groups.iter().take(10) {
        let managed = g.files.iter().filter(|f| f.managed.is_some()).count();
        let tag = if managed == g.files.len() {
            format!("[全部归 {} 管辖]", g.files[0].managed.unwrap_or(""))
        } else if managed > 0 {
            format!("[{managed}/{} 份归程序管辖]", g.files.len())
        } else {
            String::new()
        };
        println!(
            "  {:>9} × {} 份 → 可省 {:>9}  {:<28} {}",
            fmt(g.size),
            g.files.len(),
            fmt(g.reclaimable),
            g.files
                .first()
                .map(|f| f.name.chars().take(26).collect::<String>())
                .unwrap_or_default(),
            tag
        );
    }

    // ---- 抽查：随便挑几组，逐字节确认真的一模一样 ----
    println!("\n抽查（逐字节比对，确认不是哈希碰撞）:");
    let mut checked = 0;
    let mut mismatched = 0;
    for g in report.groups.iter().take(5) {
        let Some(base) = g.files.first() else {
            continue;
        };
        let Ok(a) = std::fs::read(&base.path) else {
            continue;
        };
        for other in g.files.iter().skip(1) {
            let Ok(b) = std::fs::read(&other.path) else {
                continue;
            };
            checked += 1;
            if a != b {
                mismatched += 1;
                println!("  !! 内容不同却被判为重复: {}", other.name);
            }
        }
    }
    println!("  比对 {checked} 对，不一致 {mismatched} 对");
    println!(
        "\n======== {} ========",
        if mismatched == 0 && checked > 0 {
            "通过：判定为重复的文件确实逐字节相同"
        } else if checked == 0 {
            "没找到可抽查的重复组"
        } else {
            "失败：存在误判"
        }
    );
}
