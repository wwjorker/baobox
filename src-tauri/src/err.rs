use serde::Serialize;
use std::collections::HashMap;

/// 面向用户的错误。
///
/// 方案风险 19：绝不把 Rust 的技术堆栈直接抛给用户。这里只传 i18n key
/// 和插值变量，由前端翻译成人话；技术细节放 `detail`，仅用于日志和
/// 「查看详情」，不作为主要提示。
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    /// i18n 键，如 "err.decode"
    pub key: String,
    pub vars: HashMap<String, String>,
    /// 原始技术信息，排查用
    pub detail: String,
}

impl AppError {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.into(),
            vars: HashMap::new(),
            detail: String::new(),
        }
    }

    pub fn var(mut self, k: &str, v: impl Into<String>) -> Self {
        self.vars.insert(k.into(), v.into());
        self
    }

    pub fn detail(mut self, d: impl std::fmt::Display) -> Self {
        self.detail = d.to_string();
        self
    }

    pub fn decode(format: &str, e: impl std::fmt::Display) -> Self {
        Self::new("err.decode").var("format", format).detail(e)
    }

    pub fn unknown(e: impl std::fmt::Display) -> Self {
        let msg = e.to_string();
        Self::new("err.unknown").var("detail", msg.clone()).detail(msg)
    }
}

/// 把 io 错误分流到具体的、可操作的提示上，而不是笼统的「失败了」
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind::*;
        let key = match e.kind() {
            PermissionDenied => "err.noPermission",
            NotFound => "err.notFound",
            _ => {
                // Windows 在超长路径下常报 ERROR_PATH_NOT_FOUND(3) / ERROR_FILENAME_EXCED_RANGE(206)
                match e.raw_os_error() {
                    Some(3) | Some(206) => "err.pathTooLong",
                    _ => "err.unknown",
                }
            }
        };
        Self::new(key).var("detail", e.to_string()).detail(e)
    }
}

pub type AppResult<T> = Result<T, AppError>;
