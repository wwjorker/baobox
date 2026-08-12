//! 验收：简繁转换
//!
//! 重点验「一简对多繁」——逐字映射表在这类词上必然出错，
//! 而这正是决定这个功能能不能用的地方。

use baobox_lib::textfile::convert_zh;
use opencc_fmmseg::OpenCC;

fn main() {
    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
            println!("  [OK]   {name:<26} {detail}");
        } else {
            fail += 1;
            println!("  [FAIL] {name:<26} {detail}");
        }
    };

    let tmp = std::env::temp_dir().join("baobox_zh");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let cc = OpenCC::new();

    let run = |name: &str, text: &str, hant: bool| -> String {
        let p = tmp.join(format!("{name}.txt"));
        std::fs::write(&p, text.as_bytes()).unwrap();
        let (dst, _) = convert_zh(&cc, &p, hant).unwrap();
        let b = std::fs::read(&dst).unwrap();
        String::from_utf8_lossy(&b[3..]).to_string()
    };

    println!("======== 简 → 繁 ========");

    let basic = run("basic", "这是简体中文测试", true);
    check("基本转换", basic == "這是簡體中文測試", basic.clone());

    // 一简对多繁：同一个「发」，两个词该转成不同的繁体字
    let fa = run("fa", "头发很长，发展很快", true);
    check(
        "一简对多繁·发",
        fa.contains("頭髮") && fa.contains("發展"),
        format!("「{fa}」——逐字映射表在这里必然错一个"),
    );

    let gan = run("gan", "干净的干部把树干弄干了", true);
    check(
        "一简对多繁·干",
        gan.contains("乾淨") && gan.contains("幹部"),
        format!("「{gan}」"),
    );

    let li = run("li", "里面有三里路", true);
    check(
        "一简对多繁·里",
        li.contains("裡面") || li.contains("裏面"),
        format!("「{li}」"),
    );

    println!("\n======== 繁 → 简 ========");

    // 输入说的是「繁體」，转成简体应该是「繁体」——不是「简体」
    let back = run("back", "這是繁體中文測試", false);
    check("基本转换", back == "这是繁体中文测试", back.clone());

    let back2 = run("back2", "頭髮很長，發展很快", false);
    check(
        "两个繁体字并回一个简体",
        back2.contains("头发") && back2.contains("发展"),
        format!("「{back2}」"),
    );

    println!("\n======== 边界 ========");

    let ascii = run("ascii", "Hello, world! 12345", true);
    check(
        "非中文原样不动",
        ascii == "Hello, world! 12345",
        ascii.clone(),
    );

    let p = tmp.join("same.txt");
    std::fs::write(&p, "Hello".as_bytes()).unwrap();
    let (_, changed) = convert_zh(&cc, &p, true).unwrap();
    check(
        "没变化时如实汇报",
        changed == 0,
        "界面会提示「一个字都没变」，通常意味着方向搞反了".into(),
    );

    let empty = tmp.join("empty.txt");
    std::fs::write(&empty, b"").unwrap();
    check(
        "空文件明确报错",
        convert_zh(&cc, &empty, true).is_err(),
        "err.emptyFile".into(),
    );

    // GBK 的简体老文件应该也能直接转
    let gbk = tmp.join("gbk.txt");
    std::fs::write(&gbk, vec![0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4]).unwrap();
    let (gdst, _) = convert_zh(&cc, &gbk, true).unwrap();
    let gb = std::fs::read(&gdst).unwrap();
    let gtext = String::from_utf8_lossy(&gb[3..]).to_string();
    check(
        "GBK 文件直接可转",
        gtext == "中文測試",
        format!("「{gtext}」——不用先跑一遍乱码修复"),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}
