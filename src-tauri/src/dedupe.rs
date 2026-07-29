use crate::err::{AppError, AppResult};
use crate::paths::long_path;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// 取消标志。
///
/// 实测扫描整块 1.88 TB 的机械盘要 8.9 分钟，全文件哈希阶段是 IO 密集，
/// 期间用户完全没法中断——只能等或者杀进程。一个跑十分钟又停不下来的
/// 操作是不可接受的，所以三个阶段都要能随时退出。
static CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn cancel_scan() {
    CANCEL.store(true, Ordering::Relaxed);
}

fn cancelled() -> bool {
    CANCEL.load(Ordering::Relaxed)
}

/// 重复文件查找
///
/// 三级渐进筛选，避免对整盘做全文件哈希：
///   ① 按体积分组——体积不同必然不是重复，这一步就能刷掉绝大多数
///   ② 首尾各 4 KB 快速哈希——同体积但内容不同的，通常头几个字节就分道扬镳
///   ③ 全文件 blake3——只有前两关都没分开的才需要，数量已经很少
///
/// 只看文件名是靠不住的：同一份文件改个名就认不出，
/// 而不同内容重名的情况也不少见。

const QUICK_CHUNK: usize = 4096;
/// 小于这个体积的文件不参与——几 KB 的碎文件重复了也省不下什么，
/// 却会让结果列表淹没在噪音里
const MIN_SIZE: u64 = 64 * 1024;

#[derive(Serialize, Clone)]
pub struct DupFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    /// 修改时间的 Unix 秒，用于挑「最早的那份」作为保留项
    pub modified: i64,
    /// 建议保留。每组自动留一份，其余预选待删。
    pub keep: bool,
    /// 归某个程序或环境管辖，删了会把它弄坏
    pub managed: Option<&'static str>,
}

/// 这个文件是不是由某个程序 / 包管理器 / 版本库管辖的？
///
/// 实测全盘扫描时，收益最高的几组全是这类：conda 环境里的 CUDA 运行库、
/// git 内部对象、模型缓存。它们字节完全相同，但**每一份都必须待在自己的
/// 路径上**——删掉任何一份，那个环境就废了；git 对象删了直接损坏仓库。
///
/// 「内容相同」不等于「可以删」。这类一律不预勾选，并在界面上标明归属。
fn managed_by(path: &str) -> Option<&'static str> {
    let p = path.to_ascii_lowercase().replace('/', "\\");
    const MARKERS: &[(&str, &str)] = &[
        ("\\.git\\", "Git 仓库"),
        ("\\site-packages\\", "Python 包"),
        ("\\node_modules\\", "npm 依赖"),
        ("\\.cargo\\", "Rust 依赖"),
        ("\\.conda\\", "Conda 环境"),
        ("\\anaconda", "Anaconda"),
        ("\\miniconda", "Miniconda"),
        ("\\venv\\", "虚拟环境"),
        ("\\.venv\\", "虚拟环境"),
        ("\\envs\\", "虚拟环境"),
        ("\\.m2\\", "Maven 仓库"),
        ("\\.gradle\\", "Gradle 缓存"),
        ("\\target\\debug\\", "构建产物"),
        ("\\target\\release\\", "构建产物"),
        ("\\.nuget\\", "NuGet 包"),
        ("\\appdata\\", "应用数据"),
        ("\\program files", "已安装程序"),
        ("\\windows\\", "系统文件"),
        ("\\.cache\\", "缓存目录"),
        ("\\.ollama\\", "模型缓存"),
        ("\\huggingface\\", "模型缓存"),
    ];
    MARKERS.iter().find(|(m, _)| p.contains(m)).map(|(_, label)| *label)
}

#[derive(Serialize, Clone)]
pub struct DupGroup {
    pub size: u64,
    pub files: Vec<DupFile>,
    /// 这一组删掉冗余后能省下的字节
    pub reclaimable: u64,
}

#[derive(Serialize, Clone)]
pub struct DupReport {
    pub groups: Vec<DupGroup>,
    pub scanned: usize,
    pub total_reclaimable: u64,
    /// 因权限等原因读不了的文件数，如实告知而不是静默跳过
    pub unreadable: usize,
    /// 跳过的云端占位文件数。读它们会触发下载，与腾空间的目的相悖。
    pub skipped_cloud: usize,
    /// 全组都归程序管辖、一份都不建议删的组数
    pub managed_groups: usize,
    /// 用户中途取消。结果不完整，界面上必须说明，不能当成扫完了。
    pub cancelled: bool,
}

#[derive(Serialize, Clone)]
struct ScanProgress {
    phase: &'static str,
    done: usize,
    total: usize,
}

/// 这个文件是不是云盘的「仅在线」占位符？
///
/// OneDrive、WPS 云盘这类会把文件留成占位符，**一读就触发下载**。
/// 查重工具的用途是腾空间，结果却把云端几个 GB 拉回本地，
/// 是彻底的南辕北辙。所以扫描必须绕开它们。
/// （这条是从实测里学到的——普查时我自己就把 175 份云文件拽了下来。）
#[cfg(windows)]
fn is_cloud_placeholder(attrs: u32) -> bool {
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const OFFLINE: u32 = 0x0000_1000;
    attrs & (RECALL_ON_DATA_ACCESS | RECALL_ON_OPEN | OFFLINE) != 0
}

fn quick_hash(path: &PathBuf, size: u64) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(long_path(path)).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; QUICK_CHUNK.min(size as usize)];
    f.read_exact(&mut buf).ok()?;
    hasher.update(&buf);
    if size > QUICK_CHUNK as u64 * 2 {
        f.seek(SeekFrom::End(-(QUICK_CHUNK as i64))).ok()?;
        f.read_exact(&mut buf).ok()?;
        hasher.update(&buf);
    }
    Some(*hasher.finalize().as_bytes())
}

fn full_hash(path: &PathBuf) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(long_path(path)).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(*hasher.finalize().as_bytes())
}

fn modified_secs(p: &PathBuf) -> i64 {
    std::fs::metadata(long_path(p))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn scan_blocking(app: AppHandle, roots: Vec<String>) -> DupReport {
    scan(&roots, &|phase, done, total| {
        let _ = app.emit("baobox://scan", ScanProgress { phase, done, total });
    })
}

/// 核心扫描逻辑，进度以回调形式给出，方便脱离 Tauri 直接做验收测试
pub fn scan(roots: &[String], progress: &dyn Fn(&'static str, usize, usize)) -> DupReport {
    let emit = progress;
    // 每次新扫描都清掉上一轮遗留的取消标志
    CANCEL.store(false, Ordering::Relaxed);

    // ---- 阶段 ①：遍历并按体积分组 ----
    emit("walk", 0, 0);
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut scanned = 0usize;
    let mut unreadable = 0usize;
    let mut skipped_cloud = 0usize;

    for root in roots {
        for entry in jwalk::WalkDir::new(root).skip_hidden(false) {
            if cancelled() { break; }
            let Ok(e) = entry else {
                unreadable += 1;
                continue;
            };
            if !e.file_type().is_file() {
                continue;
            }
            let p = e.path();
            let Ok(meta) = std::fs::metadata(long_path(&p)) else {
                unreadable += 1;
                continue;
            };
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if is_cloud_placeholder(meta.file_attributes()) {
                    skipped_cloud += 1;
                    continue;
                }
            }
            let size = meta.len();
            if size < MIN_SIZE {
                continue;
            }
            scanned += 1;
            if scanned % 500 == 0 {
                emit("walk", scanned, 0);
            }
            by_size.entry(size).or_default().push(p);
        }
    }

    // 体积唯一的直接排除
    let candidates: Vec<(u64, Vec<PathBuf>)> = by_size
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .collect();

    // ---- 阶段 ②：首尾快速哈希 ----
    let total_quick: usize = candidates.iter().map(|(_, v)| v.len()).sum();
    let mut done = 0usize;
    let mut by_quick: HashMap<(u64, [u8; 32]), Vec<PathBuf>> = HashMap::new();
    for (size, files) in candidates {
        for p in files {
            if cancelled() { break; }
            done += 1;
            if done % 50 == 0 {
                emit("quick", done, total_quick);
            }
            match quick_hash(&p, size) {
                Some(h) => by_quick.entry((size, h)).or_default().push(p),
                None => unreadable += 1,
            }
        }
    }

    // ---- 阶段 ③：全文件哈希，只对仍然撞在一起的 ----
    let finalists: Vec<(u64, Vec<PathBuf>)> = by_quick
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|((size, _), v)| (size, v))
        .collect();

    let total_full: usize = finalists.iter().map(|(_, v)| v.len()).sum();
    done = 0;
    let mut by_full: HashMap<(u64, [u8; 32]), Vec<PathBuf>> = HashMap::new();
    for (size, files) in finalists {
        for p in files {
            if cancelled() { break; }
            done += 1;
            if done % 20 == 0 {
                emit("full", done, total_full);
            }
            match full_hash(&p) {
                Some(h) => by_full.entry((size, h)).or_default().push(p),
                None => unreadable += 1,
            }
        }
    }

    // ---- 汇总 ----
    let mut groups: Vec<DupGroup> = Vec::new();
    let mut total_reclaimable = 0u64;
    let mut managed_groups = 0usize;
    for ((size, _), paths) in by_full {
        if paths.len() < 2 {
            continue;
        }
        let mut files: Vec<DupFile> = paths
            .into_iter()
            .map(|p| {
                let path = p.to_string_lossy().to_string();
                DupFile {
                    name: p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                    modified: modified_secs(&p),
                    managed: managed_by(&path),
                    path,
                    size,
                    keep: false,
                }
            })
            .collect();
        // 保留最早的那份——它更可能是「原件」，后来的多半是复制品
        files.sort_by_key(|f| (f.modified, f.path.clone()));
        files[0].keep = true;
        // 归程序管辖的一律标为保留，不进预选删除名单
        files.iter_mut().for_each(|f| {
            if f.managed.is_some() {
                f.keep = true;
            }
        });

        // 只统计真正建议删除的那部分，别拿一个删了会出事的数字诱导用户
        let deletable = files.iter().filter(|f| !f.keep).count() as u64;
        let reclaimable = size * deletable;
        if deletable == 0 {
            managed_groups += 1;
        }
        total_reclaimable += reclaimable;
        groups.push(DupGroup { size, files, reclaimable });
    }
    // 能省得最多的排在最前，用户从上往下处理收益最高
    groups.sort_by(|a, b| b.reclaimable.cmp(&a.reclaimable));

    emit("done", scanned, scanned);
    DupReport { groups, scanned, total_reclaimable, unreadable, skipped_cloud, managed_groups, cancelled: cancelled() }
}

#[tauri::command]
pub async fn find_duplicates(app: AppHandle, roots: Vec<String>) -> DupReport {
    tauri::async_runtime::spawn_blocking(move || scan_blocking(app, roots))
        .await
        .unwrap_or_else(|_| DupReport {
            groups: Vec::new(),
            scanned: 0,
            total_reclaimable: 0,
            unreadable: 0,
            skipped_cloud: 0,
            managed_groups: 0,
            cancelled: false,
        })
}

#[derive(Serialize, Clone)]
pub struct TrashResult {
    pub path: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// 删除到系统回收站。
///
/// 安全红线 2：绝不 `fs::remove_file`。这类工具最怕的就是手一抖，
/// 而回收站意味着任何误删都能原地还原。
#[tauri::command]
pub async fn delete_to_trash(paths: Vec<String>) -> Vec<TrashResult> {
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| match trash::delete(&p) {
                Ok(_) => TrashResult { path: p, ok: true, error: None },
                Err(e) => TrashResult { path: p, ok: false, error: Some(e.to_string()) },
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// 让前端能选文件夹作为扫描根
#[tauri::command]
pub fn dir_exists(path: String) -> AppResult<bool> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(AppError::new("err.notFound"));
    }
    Ok(p.is_dir())
}
