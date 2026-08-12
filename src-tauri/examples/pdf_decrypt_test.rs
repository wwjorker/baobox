//! 解密验收：拿真实加密 PDF 试，确认产物不再加密且页数不变。
//!
//! 现实里最常见的是「只设了权限密码、打开密码为空」的文件——
//! 能直接看但不让打印复制，这正是用户想解锁的那一类。

use baobox_lib::pdf_ops::decrypt_file;
use lopdf::Document;
use std::path::{Path, PathBuf};

/// 样本先复制到临时目录再处理。
///
/// 解密测试尤其不能直接对着真实文件跑——产物是**去掉密码的副本**，
/// 留在用户的文档目录里等于把加密保护抹掉了，若那目录还在云同步下，
/// 明文副本会直接上云。
fn stage(src: &Path, sandbox: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(sandbox).ok()?;
    let dst = sandbox.join(src.file_name()?);
    std::fs::copy(src, &dst).ok()?;
    Some(dst)
}

fn main() {
    let list = std::env::args().nth(1).unwrap_or_else(|| {
        r"C:\Users\wty\AppData\Local\Temp\claude\f--AI--\aba2743b-ab4b-4ea7-a33a-b47ab2ed99fa\scratchpad\pdf_list_real.txt".into()
    });

    // 先把加密的挑出来
    let mut encrypted: Vec<(PathBuf, usize)> = Vec::new();
    for line in std::fs::read_to_string(&list).expect("读不到清单").lines() {
        if encrypted.len() >= 10 {
            break;
        }
        let p = PathBuf::from(line.trim());
        if let Ok(doc) = Document::load(&p) {
            if doc.is_encrypted() {
                encrypted.push((p, doc.get_pages().len()));
            }
        }
    }

    println!("======== 解密验收 ========");
    println!("找到 {} 份加密 PDF\n", encrypted.len());
    if encrypted.is_empty() {
        println!("语料里没有加密文件，无法验收");
        return;
    }

    let sandbox = std::env::temp_dir().join("baobox_pdf_decrypt_test");
    let _ = std::fs::remove_dir_all(&sandbox);

    let (mut ok, mut wrong_pw, mut broken) = (0, 0, 0);
    for (p, pages) in &encrypted {
        let name: String = p
            .file_name()
            .unwrap()
            .to_string_lossy()
            .chars()
            .take(38)
            .collect();
        let Some(p) = stage(p, &sandbox) else {
            println!("  [--]   {name:<38} 无法复制到沙箱，跳过");
            continue;
        };
        match decrypt_file(&p, "") {
            Ok((dst, was)) => match Document::load(&dst) {
                Ok(d) => {
                    let still = d.is_encrypted();
                    let np = d.get_pages().len();
                    if !still && np == *pages {
                        ok += 1;
                        println!("  [OK]   {name:<38} {pages} 页 · 原本加密={was} · 产物已解锁");
                    } else {
                        broken += 1;
                        println!("  [FAIL] {name:<38} 仍加密={still} 页数 {pages}->{np}");
                    }
                }
                Err(e) => {
                    broken += 1;
                    println!("  [FAIL] {name:<38} 产物打不开: {e}");
                }
            },
            Err(e) if e.key == "err.pdfWrongPassword" => {
                wrong_pw += 1;
                println!("  [--]   {name:<38} 需要打开密码（空密码不对），按预期拒绝");
            }
            Err(e) => println!("  [--]   {name:<38} {}", e.key),
        }
    }

    println!("\n解锁成功 {ok} / 需要密码 {wrong_pw} / 产物损坏 {broken}");
}
