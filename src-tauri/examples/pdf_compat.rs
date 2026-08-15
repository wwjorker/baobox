//! 真实 PDF 兼容性普查（方案风险 4）
//!
//! 真实世界的 PDF 极其混乱：加密的、损坏的、非标准的、扫描的、
//! CJK 字体嵌入方式各异的。lopdf 解析严格，必然拒掉一批。
//! 如果用户前三次试用都报错，这个项目就死了——所以在往 lopdf 上
//! 堆 8 个工具之前，先用一批来源各异的真实文件测出实际兼容率。
//!
//! 只统计聚合数据，不打印文件名和文档内容。

use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Default)]
struct Stats {
    total: usize,
    ok: usize,
    encrypted: usize,
    empty: usize,
    failed: usize,
    pages_total: usize,
    fail_kinds: BTreeMap<String, usize>,
    slowest_ms: u128,
}

/// 把错误归成几类，便于看清失败集中在哪一类，而不是散成几百条各异的消息
fn classify(msg: &str) -> String {
    let low = msg.to_lowercase();
    if low.contains("encrypt") {
        "加密".into()
    } else if low.contains("header") || low.contains("invalid file") {
        "文件头无效".into()
    } else if low.contains("xref") || low.contains("cross") {
        "交叉引用表损坏".into()
    } else if low.contains("os error") || low.contains("io error") {
        "读取失败".into()
    } else if low.contains("dictionary") || low.contains("type") || low.contains("object") {
        "对象结构异常".into()
    } else {
        msg.chars().take(44).collect::<String>()
    }
}

fn main() {
    let list = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"C:\baobox-samples\pdf_list.txt".into());
    let paths: Vec<String> = std::fs::read_to_string(&list)
        .expect("读不到清单")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut st = Stats::default();
    let t_all = Instant::now();

    for p in &paths {
        st.total += 1;
        if std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) == 0 {
            st.empty += 1;
            st.failed += 1;
            *st.fail_kinds.entry("空文件".into()).or_default() += 1;
            continue;
        }

        let t0 = Instant::now();
        // 损坏的 PDF 有可能触发 panic 而不只是返回 Err，
        // 真实语料里这种情况必须兜住，否则一个坏文件能拖垮整批处理
        let outcome = std::panic::catch_unwind(|| {
            lopdf::Document::load(p).map(|d| (d.is_encrypted(), d.get_pages().len()))
        });

        match outcome {
            Ok(Ok((enc, pages))) => {
                st.slowest_ms = st.slowest_ms.max(t0.elapsed().as_millis());
                if enc {
                    st.encrypted += 1;
                }
                st.pages_total += pages;
                st.ok += 1;
            }
            Ok(Err(e)) => {
                st.failed += 1;
                *st.fail_kinds.entry(classify(&e.to_string())).or_default() += 1;
            }
            Err(_) => {
                st.failed += 1;
                *st.fail_kinds.entry("解析器 panic".into()).or_default() += 1;
            }
        }
    }

    let secs = t_all.elapsed().as_secs_f32();
    let pct = |n: usize| n as f32 / st.total.max(1) as f32 * 100.0;

    println!("======== 真实 PDF 兼容性普查 ========");
    println!("样本总数      {}", st.total);
    println!("解析成功      {}  ({:.1}%)", st.ok, pct(st.ok));
    println!("  其中加密    {}", st.encrypted);
    println!("  页数合计    {}", st.pages_total);
    println!("解析失败      {}  ({:.1}%)", st.failed, pct(st.failed));
    println!();
    println!("失败原因分布（决定优先修哪一类）:");
    let mut kinds: Vec<_> = st.fail_kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1));
    for (k, n) in kinds.iter().take(12) {
        println!("  {n:>5}  {k}");
    }
    println!();
    println!("总耗时 {secs:.1}s，单份最慢 {} ms", st.slowest_ms);
}
