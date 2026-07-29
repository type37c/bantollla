//! banto-kernel — 追記専用・ハッシュ連鎖・証拠付きイベント台帳のカーネル。
//!
//! 契約は docs/contract_v1.md が唯一の権威。このクレートはその実装にすぎない。
//! カーネルは意図的に小さく保つ(行数上限は tests/loc_budget.rs が強制する)。

pub mod canonical;
pub mod event;
pub mod fold;
pub mod ledger;

pub use event::{Envelope, Evidence, SCHEMA};
pub use fold::{Brief, BriefConfig, Health, LoopBrief, LoopState, LoopStatus};
pub use ledger::{Draft, Ledger, VerifyIssue, VerifyReport};

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("canonicalization error: {0}")]
    Canonical(String),
    #[error("parse error at line {line}: {msg}")]
    Parse { line: usize, msg: String },
    #[error("validation error: {0}")]
    Validation(String),
    #[error("ledger is locked by another process")]
    Locked,
    #[error("operation key conflict: {0}")]
    OpConflict(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("ledger corrupt: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, KernelError>;

/// ファイルの SHA-256 (小文字 hex) とバイト数を返す。証拠登録に使う。
pub fn hash_file(path: &Path) -> Result<(String, u64)> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex::encode(hasher.finalize()), total))
}

/// バイト列の SHA-256 (小文字 hex)。
pub fn hash_bytes(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}
