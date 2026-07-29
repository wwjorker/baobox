//! 验收：输出位置与 N→1 结果汇报
//!
//! 两条都是这次修的东西里风险最高的：
//!
//! 1. 用户可以指定输出目录。**我们自建的 Baobox_output 里可以覆盖旧产物**
//!    （否则同一批跑三遍堆出三份），但**用户自己指定的目录里绝不能覆盖**
//!    —— 那里面可能本来就有他的东西。搞反了就是删用户文件。
//!
//! 2. 合并这类 N→1 的工具，每一个输入都要有一条结果。之前只发第一条，
//!    界面上后面几行永远停在「等待」，看着像卡死。

use baobox_lib::batch::{fold_outcomes, FileOutcome};
use baobox_lib::paths::{set_output_dir, unique_path};
use std::path::PathBuf;

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {name:<24} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {name:<24} {detail}");
        }
    };

    let tmp = std::env::temp_dir().join("baobox_outdir_test");
    let _ = std::fs::remove_dir_all(&tmp);
    let ours = tmp.join("Baobox_output");
    let theirs = tmp.join("我自己的文件夹");
    std::fs::create_dir_all(&ours).unwrap();
    std::fs::create_dir_all(&theirs).unwrap();

    println!("======== 输出位置 ========");

    // --- 默认模式：我们自建的目录，同名产物直接覆盖 ---
    set_output_dir(None).unwrap();
    let a = unique_path(&ours, "结果", "png");
    std::fs::write(&a, b"first").unwrap();
    let b = unique_path(&ours, "结果", "png");
    check(
        "自建目录可覆盖",
        a == b,
        format!("两次都指向 {}", b.file_name().unwrap().to_string_lossy()),
    );

    // --- 用户指定模式：绝不覆盖，一律加后缀 ---
    set_output_dir(Some(theirs.to_string_lossy().to_string())).unwrap();
    let existing = theirs.join("结果.png");
    std::fs::write(&existing, "用户本来就有的东西".as_bytes()).unwrap();
    let c = unique_path(&theirs, "结果", "png");
    check(
        "用户目录不覆盖",
        c != existing,
        format!("避开成 {}", c.file_name().unwrap().to_string_lossy()),
    );
    check(
        "用户原文件没动",
        std::fs::read(&existing).unwrap() == "用户本来就有的东西".as_bytes(),
        "内容仍是原样".into(),
    );

    // 同名再来一次还要继续避让，不能第二次就压上去
    std::fs::write(&c, "我们的第一份产物".as_bytes()).unwrap();
    let d = unique_path(&theirs, "结果", "png");
    check(
        "用户目录连续避让",
        d != existing && d != c,
        format!("再避开成 {}", d.file_name().unwrap().to_string_lossy()),
    );

    // --- 指定一个不存在的目录必须被拒，别等整批跑完才发现写不出去 ---
    let ghost = tmp.join("根本没有这个目录");
    check(
        "拒绝不存在的目录",
        set_output_dir(Some(ghost.to_string_lossy().to_string())).is_err(),
        "返回 err.outDirMissing".into(),
    );

    set_output_dir(None).unwrap();

    println!("\n======== N→1 的结果汇报 ========");

    let srcs: Vec<PathBuf> = (1..=3).map(|i| tmp.join(format!("doc_{i}.pdf"))).collect();
    srcs.iter().for_each(|p| std::fs::write(p, b"x").unwrap());

    let merged = tmp.join("合并结果.pdf");
    std::fs::write(&merged, b"merged").unwrap();

    let head = FileOutcome::ok(&srcs[0], merged.clone(), None);
    let out = fold_outcomes(head, &srcs[1..]);
    check(
        "每个输入都有结果",
        out.len() == srcs.len(),
        format!("{} 个输入 → {} 条结果", srcs.len(), out.len()),
    );
    check(
        "没有一条停在等待",
        out.iter().all(|o| o.ok),
        format!("{} 条全部有结论", out.len()),
    );
    check(
        "产物只挂在第一条上",
        out.iter().filter(|o| o.out_path.is_some()).count() == 1,
        "其余标为已并入".into(),
    );
    // 被并入的那几条不能报成「省下了全部体积」——它们没有自己的产物
    check(
        "并入项不虚报节省",
        out[1..].iter().all(|o| o.in_bytes == o.out_bytes),
        "in == out，差值为 0".into(),
    );

    // 合并失败时，其余输入应标为「未处理」而不是「成功」
    let bad = FileOutcome::fail(&srcs[0], baobox_lib::err::AppError::new("err.decode"));
    let out2 = fold_outcomes(bad, &srcs[1..]);
    check(
        "失败时其余标未处理",
        out2.len() == 3 && out2.iter().all(|o| !o.ok),
        "3 条全部非成功".into(),
    );

    println!("\n======== 文件夹展开 ========");

    // 「一次压一整个文件夹」是说明里写着的，之前拖文件夹进来什么都不会发生
    let root = tmp.join("一批照片");
    let sub = root.join("子文件夹");
    let mine = root.join("Baobox_output");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(root.join("a.jpg"), b"x").unwrap();
    std::fs::write(root.join("b.PNG"), b"x").unwrap();
    std::fs::write(root.join("说明.txt"), b"x").unwrap();
    std::fs::write(sub.join("c.jpg"), b"x").unwrap();
    std::fs::write(mine.join("上一轮的产物.jpg"), b"x").unwrap();

    let accepts: Vec<String> = ["jpg", "jpeg", "png"].iter().map(|s| s.to_string()).collect();
    let found = tauri::async_runtime::block_on(baobox_lib::image_ops::expand_inputs(
        vec![root.to_string_lossy().to_string()],
        accepts.clone(),
    ));

    check(
        "递归收齐子目录",
        found.len() == 3,
        format!("找到 {} 个（a.jpg / b.PNG / 子文件夹里的 c.jpg）", found.len()),
    );
    check(
        "扩展名大小写不敏感",
        found.iter().any(|p| p.ends_with("b.PNG")),
        "b.PNG 被收进来了".into(),
    );
    check(
        "过滤掉不收的类型",
        !found.iter().any(|p| p.ends_with(".txt")),
        "说明.txt 没被收".into(),
    );
    // 这条最要紧：不跳过的话，跑第二遍会把上一遍的产物再压一次，越压越糊
    check(
        "跳过自己的输出目录",
        !found.iter().any(|p| p.contains("Baobox_output")),
        "上一轮的产物没被当成输入".into(),
    );

    // 直接给文件路径时该原样通过，别被目录逻辑带跑偏
    let direct = tauri::async_runtime::block_on(baobox_lib::image_ops::expand_inputs(
        vec![root.join("a.jpg").to_string_lossy().to_string()],
        accepts,
    ));
    check("单个文件原样通过", direct.len() == 1, "1 个进 1 个出".into());

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}
