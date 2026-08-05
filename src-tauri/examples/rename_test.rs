//! 批量重命名验收（全程在临时沙箱里，不碰任何真实文件）
//!
//! 重点验三件事：
//!   1. 规则叠加的结果和预期一致
//!   2. 重名冲突和非法字符被挡住，不会覆盖已有文件
//!   3. 撤销能把名字**完整**还原回去——这是最后一道防线，不能只是「大概能还原」

use baobox_lib::rename::{rename_apply, rename_preview, rename_undo, Rule};
use std::path::PathBuf;

fn setup() -> (PathBuf, Vec<String>) {
    let dir = std::env::temp_dir().join("baobox_rename_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let names = [
        "IMG_0001.JPG",
        "IMG_0002.JPG",
        "IMG_0003.JPG",
        "报告 草稿.docx",
        "report FINAL.pdf",
    ];
    let mut paths = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let p = dir.join(n);
        std::fs::write(&p, format!("content {i}")).unwrap();
        paths.push(p.to_string_lossy().to_string());
    }
    // 故意放一个占位文件，制造「目标已存在」的冲突
    std::fs::write(dir.join("photo_01.JPG"), "occupied").unwrap();
    (dir, paths)
}

fn main() {
    let (dir, paths) = setup();
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |label: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {label:<18} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {label:<18} {detail}");
        }
    };

    println!("======== 批量重命名验收 ========\n沙箱: {}\n", dir.display());

    // ---- 1. 规则叠加：去掉 IMG_ 前缀 → 全小写 → 前面加两位序号 ----
    let rules = vec![
        Rule::Replace { find: "IMG_".into(), replace: "photo_".into() },
        Rule::Case { mode: "lower".into() },
        Rule::Number { start: 1, digits: 2, prefix: true },
    ];
    let pv = rename_preview(paths.clone(), rules.clone());
    let first = pv.iter().find(|p| p.old_name == "IMG_0001.JPG").unwrap();
    check(
        "规则叠加",
        first.new_name == "01photo_0001.JPG",
        format!("{} → {}", first.old_name, first.new_name),
    );

    // ---- 2. 冲突检测：让第一个文件撞上已存在的 photo_01.JPG ----
    let clash_rules = vec![Rule::Regex {
        find: "^IMG_000\\d$".into(),
        replace: "photo_01".into(),
    }];
    let pv2 = rename_preview(paths.clone(), clash_rules);
    let clashes = pv2.iter().filter(|p| p.conflict).count();
    check(
        "冲突检测",
        clashes >= 3,
        format!("{clashes} 个被标为冲突（3 个互撞 + 目标已存在）"),
    );

    // ---- 3. 非法字符 ----
    let bad_rules = vec![Rule::Suffix { text: "?<bad>".into() }];
    let pv3 = rename_preview(paths.clone(), bad_rules);
    check(
        "非法字符",
        pv3.iter().all(|p| p.invalid),
        format!("{}/{} 被标为非法", pv3.iter().filter(|p| p.invalid).count(), pv3.len()),
    );

    // ---- 4. 执行 ----
    let before: Vec<String> = paths
        .iter()
        .map(|p| PathBuf::from(p).file_name().unwrap().to_string_lossy().to_string())
        .collect();
    let res = rename_apply(paths.clone(), rules).expect("执行失败");
    let after: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with("Baobox"))
        .collect();
    check(
        "执行",
        res.done > 0 && after.iter().any(|n| n.starts_with("01photo")),
        format!("成功 {} · 跳过 {} · 失败 {}", res.done, res.skipped, res.failed),
    );

    // ---- 5. 撤销：必须把每一个名字都还原回去 ----
    let undo = rename_undo(res.undo_log.clone()).expect("撤销失败");
    let restored = undo.restored;
    let now: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with("Baobox"))
        .collect();
    let all_back = before.iter().all(|b| now.contains(b));
    check(
        "撤销还原",
        all_back && restored == res.done && undo.failed == 0,
        format!("还原 {restored}/{} · 失败 {} · 原名全部回来: {all_back}", res.done, undo.failed),
    );

    // ---- 6. 占位文件没被覆盖 ----
    let occupied = std::fs::read_to_string(dir.join("photo_01.JPG")).unwrap_or_default();
    check(
        "未覆盖已有文件",
        occupied == "occupied",
        format!("photo_01.JPG 内容仍是 {occupied:?}"),
    );

    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    let _ = std::fs::remove_dir_all(&dir);
}
