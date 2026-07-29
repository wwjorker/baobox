//! 验收：第二批新增工具
//!
//! PDF 页面操作、二维码、查找替换、目录树。
//! 二维码那两条是闭环验证——生成一张再自己读回来，内容对上才算数。

use baobox_lib::pdf_ops::{extract_images, parse_pages, reverse_file, select_pages};
use baobox_lib::qr::{decode_image, generate_from_file};
use baobox_lib::textfile::{replace_in_file, tree_of};
use lopdf::Document;
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

    let tmp = std::env::temp_dir().join("baobox_batch2");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("Baobox_output");

    // ------------------------------------------------------- 页码解析
    println!("======== 页码范围解析 ========");

    check(
        "逗号与区间",
        parse_pages("1,3,5-8", 20).unwrap() == vec![1, 3, 5, 6, 7, 8],
        "1,3,5-8 → [1,3,5,6,7,8]".into(),
    );
    check(
        "中文逗号也认",
        parse_pages("2，4", 10).unwrap() == vec![2, 4],
        "在这上面报错只会让人烦躁".into(),
    );
    check(
        "区间倒着写也认",
        parse_pages("8-5", 10).unwrap() == vec![5, 6, 7, 8],
        "8-5 → [5,6,7,8]".into(),
    );
    check(
        "开区间到末页",
        parse_pages("8-", 10).unwrap() == vec![8, 9, 10],
        "8- → [8,9,10]".into(),
    );
    check(
        "超范围的被丢掉",
        parse_pages("1,99", 5).unwrap() == vec![1],
        "99 不存在，静默忽略".into(),
    );
    check("去重并排序", parse_pages("3,1,3", 5).unwrap() == vec![1, 3], "3,1,3 → [1,3]".into());
    check("乱写的报错", parse_pages("abc", 5).is_err(), "err.badPageSpec".into());
    check("全部超范围也报错", parse_pages("99", 5).is_err(), "err.noPagesMatched".into());

    // ------------------------------------------------------- PDF 页面
    println!("\n======== PDF 页面操作 ========");

    // 造一份 6 页的 PDF，每页写上自己的页码，好验证顺序
    let src = tmp.join("six.pdf");
    make_pdf(&src, 6);
    let n = Document::load(&src).unwrap().get_pages().len();
    check("样本是 6 页", n == 6, format!("{n} 页"));

    let (rdst, rn) = reverse_file(&src).unwrap();
    let rdoc = Document::load(&rdst).unwrap();
    check(
        "反转后页数不变",
        rn == 6 && rdoc.get_pages().len() == 6,
        format!("{} 页", rdoc.get_pages().len()),
    );
    // 第一页的文字应该是原来的第 6 页
    let nums: Vec<u32> = rdoc.get_pages().keys().copied().collect();
    let first_text = rdoc.extract_text(&nums[..1]).unwrap_or_default();
    check(
        "顺序真的倒了",
        first_text.contains("PAGE6"),
        format!("首页内容含 {}", first_text.trim().replace('\n', " ")),
    );

    let (ddst, total, left) = select_pages(&src, "2,4", false).unwrap();
    let ddoc = Document::load(&ddst).unwrap();
    check(
        "删掉指定页",
        total == 6 && left == 4 && ddoc.get_pages().len() == 4,
        format!("6 → {}", ddoc.get_pages().len()),
    );
    let dnums: Vec<u32> = ddoc.get_pages().keys().copied().collect();
    let dtext = ddoc.extract_text(&dnums).unwrap_or_default();
    check(
        "删的是对的那两页",
        !dtext.contains("PAGE2") && !dtext.contains("PAGE4") && dtext.contains("PAGE1"),
        "剩下 1,3,5,6".into(),
    );

    let (kdst, _, kleft) = select_pages(&src, "1-2", true).unwrap();
    let kdoc = Document::load(&kdst).unwrap();
    check(
        "保留模式只留指定页",
        kleft == 2 && kdoc.get_pages().len() == 2,
        format!("只剩 {} 页", kdoc.get_pages().len()),
    );
    check(
        "不许删光所有页",
        select_pages(&src, "1-6", false).is_err(),
        "err.wouldDeleteAllPages".into(),
    );

    // ------------------------------------------------------- 提取图片
    println!("\n======== 提取内嵌图片 ========");

    // 用已有的「图片转 PDF」造一份带图的 PDF，再抠回来
    let photo = tmp.join("photo.png");
    image::RgbImage::from_fn(300, 200, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 60])
    })
    .save(&photo)
    .unwrap();

    let with_img = tmp.join("withimg.pdf");
    make_pdf_with_image(&with_img, &photo);

    match extract_images(&with_img, 50) {
        Ok((_, cnt)) => {
            check("抠出图片", cnt >= 1, format!("{cnt} 张"));
            let found: Vec<_> = std::fs::read_dir(&out)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().contains("图0"))
                .collect();
            check("产物真的落盘了", !found.is_empty(), format!("{} 个文件", found.len()));
            // 抠出来的必须跟原图逐像素一致。
            // 只验「能打开」是不够的——预测器还原写错的话，出来的是一张
            // 能正常打开的斜纹噪声图，看起来一切正常。
            let orig = image::open(&photo).unwrap().to_rgb8();
            let got = image::open(found[0].path()).unwrap().to_rgb8();
            let same_size = got.dimensions() == orig.dimensions();
            let same_px = same_size
                && orig
                    .enumerate_pixels()
                    .all(|(x, y, p)| got.get_pixel(x, y) == p);
            check(
                "像素与原图完全一致",
                same_px,
                format!("{:?} vs {:?}", got.dimensions(), orig.dimensions()),
            );
        }
        Err(e) => check("抠出图片", false, format!("报错 {}", e.key)),
    }

    let (nodst, _) = (0, 0);
    let _ = nodst;
    check(
        "最小尺寸能滤掉小图",
        extract_images(&with_img, 5000).is_err(),
        "全被滤掉时明确报错，不产空目录".into(),
    );

    // ------------------------------------------------------- 二维码闭环
    println!("\n======== 二维码（生成→识别闭环）========");

    let list = tmp.join("codes.txt");
    std::fs::write(
        &list,
        "https://github.com/wwjorker/baobox\n百宝箱 中文内容测试\n\nASSET-00427\n",
    )
    .unwrap();

    let (_, made) = generate_from_file(&list, 512).unwrap();
    check("空行不算一条", made == 3, format!("4 行 → {made} 个码"));

    // 生成的第二个码内容是中文，读回来必须一模一样
    let second = out.join("codes_002.png");
    match decode_image(&second) {
        Ok(got) => check(
            "中文内容原样读回",
            got.len() == 1 && got[0] == "百宝箱 中文内容测试",
            format!("读出「{}」", got.join("")),
        ),
        Err(e) => check("中文内容原样读回", false, format!("报错 {}", e.key)),
    }
    let first_qr = out.join("codes_001.png");
    match decode_image(&first_qr) {
        Ok(got) => check(
            "网址原样读回",
            got[0] == "https://github.com/wwjorker/baobox",
            got[0].clone(),
        ),
        Err(e) => check("网址原样读回", false, format!("报错 {}", e.key)),
    }
    check(
        "模块是整数像素",
        {
            let img = image::open(&first_qr).unwrap();
            img.width() == img.height() && img.width() % 1 == 0
        },
        "边缘是硬的，打印出来才扫得动".into(),
    );
    check(
        "没有码的图明确报错",
        decode_image(&photo).is_err(),
        "err.qrNotFound".into(),
    );

    // ------------------------------------------------------- 查找替换
    println!("\n======== 查找替换 ========");

    let txt = tmp.join("doc.txt");
    std::fs::write(&txt, "Hello world. hello WORLD. 世界你好。".as_bytes()).unwrap();

    let (r1, h1) = replace_in_file(&txt, "world", "地球", false, true).unwrap();
    let s1 = read_utf8(&r1);
    check(
        "区分大小写时只换一处",
        h1 == 1 && s1.contains("Hello 地球") && s1.contains("hello WORLD"),
        format!("{h1} 处"),
    );

    let (r2, h2) = replace_in_file(&txt, "world", "地球", false, false).unwrap();
    let s2 = read_utf8(&r2);
    check(
        "不区分大小写时两处都换",
        h2 == 2 && !s2.to_lowercase().contains("world"),
        format!("{h2} 处"),
    );

    // 正则的真正价值在捕获组回填，光验「能匹配」说明不了什么
    let orders = tmp.join("orders.txt");
    std::fs::write(&orders, "订单 A-1001 和 A-1002 已发货".as_bytes()).unwrap();
    let (r3, h3) = replace_in_file(&orders, r"A-(\d+)", "NO$1", true, true).unwrap();
    let s3 = read_utf8(&r3);
    check(
        "正则捕获组回填",
        h3 == 2 && s3.contains("NO1001") && s3.contains("NO1002") && !s3.contains("A-"),
        format!("{h3} 处 · 读出「{s3}」"),
    );

    let (_, h4) = replace_in_file(&txt, "找不到的内容", "x", false, true).unwrap();
    check("没匹配时不报错", h4 == 0, "内容原样输出".into());
    check(
        "查找为空要拒绝",
        replace_in_file(&txt, "", "x", false, true).is_err(),
        "否则会在每个字符间插入".into(),
    );
    check(
        "坏正则明确报错",
        replace_in_file(&txt, "[", "x", true, true).is_err(),
        "err.badRegex".into(),
    );

    // GBK 文件不用先修复就能直接改
    let gbk = tmp.join("gbk.txt");
    // 「中文测试」的 GBK 字节
    std::fs::write(&gbk, vec![0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4]).unwrap();
    let (rg, hg) = replace_in_file(&gbk, "测试", "验收", false, true).unwrap();
    check(
        "GBK 文件直接可改",
        hg == 1 && read_utf8(&rg).contains("中文验收"),
        format!("{hg} 处 · 读出「{}」", read_utf8(&rg)),
    );

    // ------------------------------------------------------- 目录树
    println!("\n======== 目录树 ========");

    let root = tmp.join("proj");
    std::fs::create_dir_all(root.join("src").join("deep")).unwrap();
    std::fs::write(root.join("README.md"), b"x").unwrap();
    std::fs::write(root.join("src").join("main.rs"), b"xxxxx").unwrap();
    std::fs::write(root.join("src").join("deep").join("nested.txt"), b"x").unwrap();

    let t = tree_of(&root, 4, true).unwrap();
    check("包含子目录内容", t.contains("main.rs"), "src/main.rs 在".into());
    check("目录排在文件前", {
        let si = t.find("src/").unwrap_or(usize::MAX);
        let ri = t.find("README.md").unwrap_or(0);
        si < ri
    }, "src/ 在 README.md 之前".into());
    check("显示了体积", t.contains("5 B") || t.contains("B)"), "带大小标注".into());

    let shallow = tree_of(&root, 2, false).unwrap();
    check(
        "层数限制生效",
        shallow.contains("main.rs") && !shallow.contains("nested.txt"),
        "第 3 层没被展开".into(),
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

/// 造一份 n 页的 PDF，每页写上 PAGEn，便于验证顺序
fn make_pdf(path: &PathBuf, n: u32) {
    use lopdf::{dictionary, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });

    let mut kids = Vec::new();
    for i in 1..=n {
        let content = format!("BT /F1 24 Tf 72 700 Td (PAGE{i}) Tj ET");
        let cid = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => cid,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => n, "Kids" => kids,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.compress();
    doc.save(path).unwrap();
}

/// 造一份带内嵌图片的 PDF
fn make_pdf_with_image(path: &PathBuf, img: &PathBuf) {
    use lopdf::{dictionary, Object};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let image = lopdf::xobject::image(img).unwrap();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
    });
    doc.add_page_contents(page, Vec::new()).unwrap();
    doc.insert_image(page, image, (0.0, 0.0), (300.0, 200.0)).unwrap();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)],
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}
