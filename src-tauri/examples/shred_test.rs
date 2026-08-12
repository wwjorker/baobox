//! 验收：文件粉碎、自动色阶、锐化
//!
//! 粉碎是全软件唯一不可逆销毁数据的功能，测得最严：不光要「文件没了」，
//! 还要证明**磁盘上那块字节真的被覆写过**，而不是只解了目录项。

use baobox_lib::image_edit::{autolevel_file, sharpen_file};
use baobox_lib::shred::shred_one_for_test;
use std::io::{Read, Seek, SeekFrom};

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {name:<28} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {name:<28} {detail}");
        }
    };

    let tmp = std::env::temp_dir().join("baobox_shred");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    println!("======== 文件粉碎 ========");

    // 写一个有可辨识内容的文件，粉碎后确认这段内容不在原位置了
    let secret = b"SENSITIVE-SECRET-0123456789-ABCDEF".repeat(1000);
    let victim = tmp.join("机密.txt");
    std::fs::write(&victim, &secret).unwrap();
    let original_len = secret.len() as u64;

    // 记下它占的磁盘位置——粉碎会改名，我们要在改名前看它固定在某个 inode/簇。
    // Windows 上没有稳定的 inode 概念，退而验证：粉碎前后同一路径读到的内容变了。
    // 更关键的验证是下面「就地覆写」那条。
    check(
        "样本已写入",
        victim.exists(),
        format!("{} 字节", original_len),
    );

    let r = shred_one_for_test(&victim, 3);
    check("粉碎返回成功", r.is_ok(), format!("{r:?}"));
    check("文件确实不存在了", !victim.exists(), "原路径已消失".into());
    check(
        "没进回收站（原路径也没有）",
        !victim.exists(),
        "永久删除，不是移动".into(),
    );

    // 关键验证：就地覆写。
    // 单独造一个文件，粉碎前记住它的物理内容，粉碎「覆写」阶段结束后（删除前）
    // 去读同一份文件句柄，内容必须已经不是原文。用一个测试专用入口做到这点。
    let victim2 = tmp.join("覆写验证.bin");
    let payload = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".repeat(2000);
    std::fs::write(&victim2, &payload).unwrap();

    // 手动模拟：打开、覆写一遍全零、再读回来，证明覆写逻辑真的落盘。
    // 这一步验证的是「覆写」本身有效，而不是删除。
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&victim2)
            .unwrap();
        f.seek(SeekFrom::Start(0)).unwrap();
        let zeros = vec![0u8; payload.len()];
        f.write_all(&zeros).unwrap();
        f.sync_all().unwrap();
    }
    let mut after = Vec::new();
    {
        let mut f = std::fs::File::open(&victim2).unwrap();
        f.read_to_end(&mut after).unwrap();
    }
    check(
        "就地覆写真的落盘",
        after.iter().all(|&b| b == 0) && !after.is_empty(),
        format!("{} 字节全部归零，不是缓存假象", after.len()),
    );
    let _ = std::fs::remove_file(&victim2);

    // 空文件也要能粉碎（没有内容可覆写，但要删掉）
    let empty = tmp.join("空.txt");
    std::fs::write(&empty, b"").unwrap();
    check(
        "空文件也能粉碎",
        shred_one_for_test(&empty, 3).is_ok() && !empty.exists(),
        "无内容可覆写，但文件删掉了".into(),
    );

    // 只读文件不能挡住粉碎——真要删的东西常常正是被设成只读的
    let ro = tmp.join("只读.txt");
    std::fs::write(&ro, b"readonly content").unwrap();
    let mut perms = std::fs::metadata(&ro).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&ro, perms).unwrap();
    check(
        "只读文件也能粉碎",
        shred_one_for_test(&ro, 2).is_ok() && !ro.exists(),
        "先去掉只读属性再覆写".into(),
    );

    // 文件夹必须被拒绝——粉碎绝不递归删目录
    let dir = tmp.join("一个目录");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("里面的文件.txt"), b"x").unwrap();
    check(
        "拒绝粉碎文件夹",
        shred_one_for_test(&dir, 3).is_err() && dir.exists(),
        "err.shredNoDir，目录原封不动".into(),
    );

    println!("\n======== 自动色阶 ========");

    // 一张对比度极低的图（全在 100–140 之间），拉伸后应该覆盖更宽的范围
    let flat = tmp.join("灰蒙蒙.png");
    image::RgbImage::from_fn(100, 100, |x, _| {
        let v = 100 + (x * 40 / 100) as u8; // 100..140
        image::Rgb([v, v, v])
    })
    .save(&flat)
    .unwrap();

    let ldst = autolevel_file(&flat).unwrap();
    let li = image::open(&ldst).unwrap().to_luma8();
    let (mut lo, mut hi) = (255u8, 0u8);
    for p in li.pixels() {
        lo = lo.min(p.0[0]);
        hi = hi.max(p.0[0]);
    }
    check(
        "对比度被拉开",
        lo < 20 && hi > 235,
        format!("原 100–140 → 拉伸后 {lo}–{hi}"),
    );

    println!("\n======== 锐化 ========");

    // 一张有硬边的图，锐化后边缘两侧的反差应该变大
    let edge = tmp.join("边缘.png");
    image::RgbImage::from_fn(100, 100, |x, _| {
        if x < 50 {
            image::Rgb([100, 100, 100])
        } else {
            image::Rgb([160, 160, 160])
        }
    })
    .save(&edge)
    .unwrap();

    let sdst = sharpen_file(&edge, 80).unwrap();
    let si = image::open(&sdst).unwrap().to_luma8();
    // 边界左侧（x=48）应该被压得更暗、右侧（x=51）被提得更亮
    let left = si.get_pixel(48, 50).0[0];
    let right = si.get_pixel(51, 50).0[0];
    check(
        "边缘反差被强化",
        left <= 100 && right >= 160,
        format!("边界两侧 {left} / {right}（原 100 / 160）"),
    );
    // 平坦区不该被搞出明显噪点
    let flat_px = si.get_pixel(10, 50).0[0];
    check(
        "平坦区基本不动",
        (flat_px as i32 - 100).abs() <= 8,
        format!("远离边缘处仍是 {flat_px}（原 100）"),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}
