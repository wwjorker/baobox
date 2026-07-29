//! 验收：第五批（拼版、空白页、比例裁切、主色、Base64、CSV/JSON、时间戳）

use baobox_lib::image_edit::{aspect_file, base64_of, palette_of};
use baobox_lib::pdf_ops::{insert_blank, nup_file};
use baobox_lib::textfile::{csv_to_json, json_to_csv, set_times};
use lopdf::{dictionary, Document, Object};
use std::path::PathBuf;

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

    let tmp = std::env::temp_dir().join("baobox_batch5");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // ---------------------------------------------------------- 拼版
    println!("======== N 合 1 拼版 ========");

    let six = tmp.join("six.pdf");
    make_pdf(&six, 6);

    let (dst, total, sheets) = nup_file(&six, 2, 1, 10.0).unwrap();
    let doc = Document::load(&dst).unwrap();
    check(
        "6 页 → 3 张",
        total == 6 && sheets == 3 && doc.get_pages().len() == 3,
        format!("{} 张", doc.get_pages().len()),
    );
    // 每张纸上要有两个 Form XObject 被画出来，否则就是空白页
    let (forms, draws) = count_forms(&doc);
    check(
        "每页内容真的被贴上去了",
        forms >= 6 && draws >= 6,
        format!("{forms} 个 Form · {draws} 次绘制"),
    );
    check(
        "页面尺寸没变",
        page_size(&doc) == (595.0, 842.0),
        format!("{:?}", page_size(&doc)),
    );

    let (d4, _, s4) = nup_file(&six, 2, 2, 6.0).unwrap();
    check(
        "4 合 1 是 2 张",
        s4 == 2 && Document::load(&d4).unwrap().get_pages().len() == 2,
        format!("{s4} 张"),
    );
    check(
        "1 合 1 被拒绝",
        nup_file(&six, 1, 1, 0.0).is_err(),
        "err.nupTooSmall".into(),
    );

    // ---------------------------------------------------------- 空白页
    println!("\n======== 插入空白页 ========");

    let (bdst, n) = insert_blank(&six, "2,4", 1).unwrap();
    let bdoc = Document::load(&bdst).unwrap();
    check(
        "插了 2 张",
        n == 2 && bdoc.get_pages().len() == 8,
        format!("6 页 → {} 页", bdoc.get_pages().len()),
    );
    // 第 3 页（原第 2 页之后）应该是空的
    let nums: Vec<u32> = bdoc.get_pages().keys().copied().collect();
    let third = bdoc.extract_text(&nums[2..3]).unwrap_or_default();
    check(
        "插在了正确的位置",
        third.trim().is_empty(),
        format!("第 3 页内容「{}」", third.trim()),
    );

    let (adst, an) = insert_blank(&six, "", 1).unwrap();
    check(
        "留空就是每页后都插",
        an == 6 && Document::load(&adst).unwrap().get_pages().len() == 12,
        format!("插了 {an} 张"),
    );

    // ---------------------------------------------------------- 比例裁切
    println!("\n======== 按比例裁切 ========");

    let wide = tmp.join("wide.png");
    image::RgbImage::from_pixel(1600, 900, image::Rgb([120, 80, 200]))
        .save(&wide)
        .unwrap();

    let (s1, w1, h1, nw1, nh1) = aspect_file(&wide, "1:1").unwrap();
    check(
        "16:9 裁成 1:1",
        nw1 == 900 && nh1 == 900,
        format!("{w1}×{h1} → {nw1}×{nh1}"),
    );
    check(
        "产物尺寸与报告一致",
        {
            let i = image::open(&s1).unwrap();
            i.width() == nw1 && i.height() == nh1
        },
        "读回来核对过".into(),
    );

    let (_, _, _, nw2, nh2) = aspect_file(&wide, "9:16").unwrap();
    check(
        "裁成竖版",
        nh2 > nw2 && (nw2 as f32 / nh2 as f32 - 9.0 / 16.0).abs() < 0.01,
        format!("{nw2}×{nh2}"),
    );

    let (_, _, _, nw3, nh3) = aspect_file(&wide, "16:9").unwrap();
    check(
        "本来就是这个比例时几乎不裁",
        nw3 == 1600 && nh3 == 900,
        format!("{nw3}×{nh3}"),
    );

    // ---------------------------------------------------------- 主色调
    println!("\n======== 主色调提取 ========");

    // 七成红、三成蓝，主色必须是红且占比接近 70%
    let two = tmp.join("two.png");
    image::RgbImage::from_fn(100, 100, |_, y| {
        if y < 70 {
            image::Rgb([220, 20, 20])
        } else {
            image::Rgb([20, 20, 220])
        }
    })
    .save(&two)
    .unwrap();

    let pal = palette_of(&two, 5).unwrap();
    check("取到颜色", !pal.is_empty(), format!("{} 个", pal.len()));
    let (top, pct) = &pal[0];
    // 桶中心值会有几个色阶的偏移，判红色分量占主导即可
    let r = u8::from_str_radix(&top[1..3], 16).unwrap();
    let b = u8::from_str_radix(&top[5..7], 16).unwrap();
    check(
        "主色是占比大的那个",
        r > 200 && b < 60,
        format!("{top} 占 {pct:.0}%"),
    );
    check("占比接近七成", (*pct - 70.0).abs() < 8.0, format!("{pct:.1}%"));

    // ---------------------------------------------------------- Base64
    println!("\n======== 转 Base64 ========");

    let (b64dst, uri, raw) = base64_of(&two).unwrap();
    check(
        "前缀带正确的 MIME",
        uri.starts_with("data:image/png;base64,"),
        uri.chars().take(30).collect(),
    );
    // Base64 是 4/3 膨胀，对不上说明编码有问题
    let payload = uri.len() - "data:image/png;base64,".len();
    check(
        "长度符合 4/3 膨胀",
        (payload as f64 / raw as f64 - 4.0 / 3.0).abs() < 0.02,
        format!("{raw} 字节 → {payload} 字符"),
    );
    check(
        "同时落了一份 txt",
        b64dst.exists() && std::fs::read_to_string(&b64dst).unwrap() == uri,
        "几十 KB 的串没法在界面里读完".into(),
    );

    // ---------------------------------------------------------- CSV / JSON
    println!("\n======== CSV ↔ JSON ========");

    let csv = tmp.join("data.csv");
    std::fs::write(
        &csv,
        "姓名,备注,金额\r\n张三,\"含,逗号\",100\r\n李四,\"带\"\"引号\"\"\",200\r\n".as_bytes(),
    )
    .unwrap();

    let (jdst, jn) = csv_to_json(&csv, true).unwrap();
    let jtext = read_utf8(&jdst);
    let parsed: serde_json::Value = serde_json::from_str(&jtext).unwrap();
    check("转出 2 条记录", jn == 2, format!("{jn} 条"));
    check(
        "引号里的逗号没被当分隔符",
        parsed[0]["备注"] == "含,逗号",
        format!("读出「{}」", parsed[0]["备注"].as_str().unwrap_or("")),
    );
    check(
        "双写引号还原成一个",
        parsed[1]["备注"] == "带\"引号\"",
        format!("读出「{}」", parsed[1]["备注"].as_str().unwrap_or("")),
    );

    // 转回去，特殊字符必须重新被正确转义
    let (cdst, cn) = json_to_csv(&jdst).unwrap();
    let ctext = read_utf8(&cdst);
    check("转回 2 行", cn == 2, format!("{cn} 行"));
    check(
        "特殊字符重新转义",
        ctext.contains("\"含,逗号\"") && ctext.contains("\"\"引号\"\""),
        "闭环回来没有丢格式".into(),
    );
    check(
        "带 BOM 给 Excel",
        std::fs::read(&cdst).unwrap().starts_with(&[0xEF, 0xBB, 0xBF]),
        "否则中文列名在 Excel 里又是乱码".into(),
    );

    let bad = tmp.join("bad.json");
    std::fs::write(&bad, b"{not json").unwrap();
    check("坏 JSON 明确报错", json_to_csv(&bad).is_err(), "err.badJson".into());

    // ---------------------------------------------------------- 时间戳
    println!("\n======== 批量改时间 ========");

    let tf = tmp.join("stamp.bin");
    std::fs::write(&tf, b"content").unwrap();
    let before_meta = std::fs::metadata(&tf).unwrap();
    let before = before_meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let after = set_times(&tf, -8, None).unwrap();
    let now = std::fs::metadata(&tf)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    check(
        "时间平移了 8 小时",
        (before - now - 8 * 3600).abs() <= 2 && after == now,
        format!("{before} → {now}"),
    );
    check(
        "内容一个字节没动",
        std::fs::read(&tf).unwrap() == b"content",
        "改的是属性不是内容".into(),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n======== 通过 {pass} / 失败 {fail} ========");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn read_utf8(p: &PathBuf) -> String {
    let b = std::fs::read(p).unwrap();
    let body = if b.starts_with(&[0xEF, 0xBB, 0xBF]) { &b[3..] } else { &b[..] };
    String::from_utf8_lossy(body).to_string()
}

/// 数出文档里的 Form XObject 个数，以及内容流里 Do 指令的次数
fn count_forms(doc: &Document) -> (usize, usize) {
    let forms = doc
        .objects
        .values()
        .filter(|o| {
            matches!(o, Object::Stream(s) if s.dict.get(b"Subtype")
                .and_then(|x| x.as_name()).map(|n| n == b"Form").unwrap_or(false))
        })
        .count();
    let draws: usize = doc
        .objects
        .values()
        .filter_map(|o| match o {
            Object::Stream(s) => s.get_plain_content().ok(),
            _ => None,
        })
        .map(|c| String::from_utf8_lossy(&c).matches(" Do").count())
        .sum();
    (forms, draws)
}

fn page_size(doc: &Document) -> (f32, f32) {
    let Some((_, id)) = doc.get_pages().into_iter().next() else {
        return (0.0, 0.0);
    };
    let Ok(d) = doc.get_object(id).and_then(|o| o.as_dict()) else {
        return (0.0, 0.0);
    };
    let Ok(a) = d.get(b"MediaBox").and_then(|o| o.as_array()) else {
        return (0.0, 0.0);
    };
    let v: Vec<f32> = a
        .iter()
        .filter_map(|o| match o {
            Object::Integer(i) => Some(*i as f32),
            Object::Real(f) => Some(*f),
            _ => None,
        })
        .collect();
    if v.len() == 4 {
        (v[2] - v[0], v[3] - v[1])
    } else {
        (0.0, 0.0)
    }
}

fn make_pdf(path: &PathBuf, n: u32) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
    let mut kids = Vec::new();
    for i in 1..=n {
        let content = format!("BT /F1 24 Tf 72 700 Td (PAGE{i}) Tj ET");
        let cid = doc.add_object(lopdf::Stream::new(dictionary! {}, content.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => cid,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Count" => n, "Kids" => kids }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}
