//! 台帳ファイル。JSONL 追記専用、ロック + fsync + 冪等キー。
//!
//! 個人規模を前提に、追記も検証も全読みで実装する(数万イベントまで問題ない)。
//! 賢くする前に測る。

use crate::event::{Envelope, Evidence};
use crate::{KernelError, Result, SCHEMA};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_STALE: Duration = Duration::from_secs(60);

/// 追記の下書き。seq/prev/id はカーネルが決める。
#[derive(Debug, Clone)]
pub struct Draft {
    pub ts: Option<String>,
    pub actor: String,
    pub event_type: String,
    pub body: Value,
    pub evidence: Vec<Evidence>,
    pub op: Option<String>,
}

pub struct Ledger {
    path: PathBuf,
}

struct LockGuard {
    path: PathBuf,
    token: String,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // 自分の札が入っている時だけ消す。stale 奪取の競合で他プロセスが
        // 取得し直したロックを、退出者が巻き添えで消してはならない。
        if fs::read_to_string(&self.path).is_ok_and(|s| s == self.token) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Ledger {
    /// 新しい台帳ファイルを作る。既存なら失敗する。
    pub fn create(path: &Path) -> Result<Ledger> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Ledger {
            path: path.to_path_buf(),
        })
    }

    pub fn open(path: &Path) -> Result<Ledger> {
        if !path.is_file() {
            return Err(KernelError::NotFound(format!(
                "ledger not found: {}",
                path.display()
            )));
        }
        Ok(Ledger {
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> Result<LockGuard> {
        let lock_path = PathBuf::from(format!("{}.lock", self.path.display()));
        let token = format!(
            "{}:{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        );
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    let _ = write!(file, "{token}");
                    return Ok(LockGuard {
                        path: lock_path,
                        token,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&lock_path)
                        .and_then(|m| m.modified())
                        .map(|t| t.elapsed().unwrap_or_default() > LOCK_STALE)
                        .unwrap_or(false);
                    if stale {
                        // 削除→再作成の隙間で複数プロセスが取り合わないよう、
                        // rename の原子性で唯一の奪取者を決める(敗者は再試行へ)。
                        let claim = PathBuf::from(format!("{}.{token}", lock_path.display()));
                        if fs::rename(&lock_path, &claim).is_ok() {
                            let _ = fs::remove_file(&claim);
                        }
                        continue;
                    }
                    if Instant::now() > deadline {
                        return Err(KernelError::Locked);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn read_raw(&self) -> Result<String> {
        let mut raw = String::new();
        File::open(&self.path)?.read_to_string(&mut raw)?;
        Ok(raw)
    }

    fn parse(raw: &str) -> Result<Vec<Envelope>> {
        let mut events = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            events.push(Envelope::from_line(line, i + 1)?);
        }
        Ok(events)
    }

    /// 全イベントを読み、構造を検査して返す(連鎖の完全検査は verify)。
    pub fn events(&self) -> Result<Vec<Envelope>> {
        Self::parse(&self.read_raw()?)
    }

    pub fn find(&self, id: &str) -> Result<Envelope> {
        self.events()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| KernelError::NotFound(format!("event {id}")))
    }

    /// 契約 v1 に従って1件追記する。`op` があれば冪等。
    pub fn append(&self, draft: Draft) -> Result<Envelope> {
        let _guard = self.lock()?;
        let raw = self.read_raw()?;
        let existing = Self::parse(&raw)?;
        // 末尾改行が欠けた正当なファイル(エディタ・git・中断)に連結してしまうと
        // 台帳全体が読めなくなる。欠けていれば改行を補ってから書く。
        let needs_leading_newline = !raw.is_empty() && !raw.ends_with('\n');

        if let Some(op) = &draft.op {
            if let Some(prior) = existing.iter().find(|e| e.op.as_deref() == Some(op)) {
                let candidate = Envelope {
                    schema: SCHEMA.into(),
                    seq: prior.seq,
                    prev: prior.prev.clone(),
                    ts: prior.ts.clone(),
                    actor: draft.actor.clone(),
                    event_type: draft.event_type.clone(),
                    body: draft.body.clone(),
                    evidence: draft.evidence.clone(),
                    op: Some(op.clone()),
                    id: prior.id.clone(),
                };
                if prior.same_content(&candidate) {
                    return Ok(prior.clone());
                }
                return Err(KernelError::OpConflict(op.clone()));
            }
        }

        let (seq, prev) = match existing.last() {
            None => (0, None),
            Some(last) => {
                // 末尾の完全性だけは追記のたびに確かめる(全検査は verify)。
                last.validate()?;
                (last.seq + 1, Some(last.id.clone()))
            }
        };

        let mut env = Envelope {
            schema: SCHEMA.into(),
            seq,
            prev,
            ts: draft.ts.unwrap_or_else(now_rfc3339),
            actor: draft.actor,
            event_type: draft.event_type,
            body: draft.body,
            evidence: draft.evidence,
            op: draft.op,
            id: String::new(),
        };
        env.id = env.compute_id()?;
        env.validate()?;

        let line = env.to_line()?;
        // 書けるのに二度と読めない行(例: serde_json の再帰上限を超える深い body)を
        // 通すと、追記専用ゆえ台帳が恒久的に自壊する。書く前に必ず読み戻す。
        if let Err(e) = Envelope::from_line(&line, existing.len() + 1) {
            return Err(KernelError::Validation(format!(
                "refusing to append a line that cannot be read back: {e}"
            )));
        }
        let mut out = String::with_capacity(line.len() + 2);
        if needs_leading_newline {
            out.push('\n');
        }
        out.push_str(&line);
        out.push('\n');
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        file.write_all(out.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        if let Some(parent) = self.path.parent() {
            if let Ok(dir) = File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(env)
    }

    /// 台帳全体を検査する。すべての問題を集めて返す(最初の1件で止まらない)。
    pub fn verify(&self, evidence_root: Option<&Path>, deep: bool) -> Result<VerifyReport> {
        let raw = self.read_raw()?;
        let mut issues = Vec::new();
        let mut prev_id: Option<String> = None;
        let mut expected_seq: u64 = 0;
        let mut seen_ops: std::collections::HashMap<String, u64> = Default::default();
        let mut count = 0usize;

        // str::lines() は行末の \r を黙って剥がすため CRLF 一括変換が見えなくなる。
        // 素の split で読み、\r は non-canonical として鳴らしてから続きを検査する。
        let mut lines: Vec<&str> = raw.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop(); // 末尾改行の後ろの空要素はファイル終端であって空行ではない
        }
        for (i, mut line) in lines.into_iter().enumerate() {
            let line_no = i + 1;
            if let Some(stripped) = line.strip_suffix('\r') {
                issues.push(VerifyIssue::new(
                    line_no,
                    "non-canonical",
                    "line ends with CR: ledger must be LF-only (check git autocrlf)",
                ));
                line = stripped;
            }
            if line.is_empty() {
                issues.push(VerifyIssue::new(
                    line_no,
                    "empty-line",
                    "empty line in ledger",
                ));
                continue;
            }
            let env = match Envelope::from_line(line, line_no) {
                Ok(env) => env,
                Err(e) => {
                    issues.push(VerifyIssue::new(line_no, "parse", &e.to_string()));
                    continue;
                }
            };
            count += 1;
            if let Err(e) = env.validate() {
                issues.push(VerifyIssue::new(line_no, "validation", &e.to_string()));
            }
            match env.to_line() {
                Ok(canon) if canon != line => {
                    issues.push(VerifyIssue::new(
                        line_no,
                        "non-canonical",
                        "line is not the canonical serialization of its content",
                    ));
                }
                Err(e) => issues.push(VerifyIssue::new(line_no, "canonical", &e.to_string())),
                _ => {}
            }
            if env.seq != expected_seq {
                issues.push(VerifyIssue::new(
                    line_no,
                    "seq",
                    &format!("expected seq {expected_seq}, got {}", env.seq),
                ));
            }
            if env.prev != prev_id {
                issues.push(VerifyIssue::new(
                    line_no,
                    "chain",
                    "prev does not match previous id",
                ));
            }
            if let Some(op) = &env.op {
                if let Some(first) = seen_ops.insert(op.clone(), env.seq) {
                    issues.push(VerifyIssue::new(
                        line_no,
                        "op-duplicate",
                        &format!("op {op} already used at seq {first}"),
                    ));
                }
            }
            if let Some(root) = evidence_root {
                for entry in &env.evidence {
                    if let Some(uri) = &entry.uri {
                        // 絶対パス uri は「ワークスペース外の証拠」の正直な形式として許す
                        // (cli_spec)。欺瞞的なのは root 相対に見えて外へ出る「..」で、
                        // これは任意ファイルへのハッシュ照合(オラクル)になるため
                        // 検査せず invalid として鳴らす。
                        let p = Path::new(uri);
                        if !p.is_absolute()
                            && p.components()
                                .any(|c| matches!(c, std::path::Component::ParentDir))
                        {
                            issues.push(VerifyIssue::new(
                                line_no,
                                "evidence-invalid",
                                &format!("evidence uri escapes evidence root: {uri}"),
                            ));
                            continue;
                        }
                        let path = root.join(uri);
                        if !path.is_file() {
                            issues.push(VerifyIssue::new(
                                line_no,
                                "evidence-missing",
                                &format!("evidence file not found: {uri}"),
                            ));
                        } else if deep {
                            match crate::hash_file(&path) {
                                Ok((hash, _)) if hash != entry.sha256 => {
                                    issues.push(VerifyIssue::new(
                                        line_no,
                                        "evidence-hash",
                                        &format!("evidence hash mismatch: {uri}"),
                                    ));
                                }
                                Err(e) => issues.push(VerifyIssue::new(
                                    line_no,
                                    "evidence-read",
                                    &format!("{uri}: {e}"),
                                )),
                                _ => {}
                            }
                        }
                    }
                }
            }
            prev_id = Some(env.id.clone());
            expected_seq = env.seq.saturating_add(1);
        }

        Ok(VerifyReport {
            events: count,
            issues,
        })
    }
}

pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

#[derive(Debug, Clone)]
pub struct VerifyIssue {
    pub line: usize,
    pub class: String,
    pub message: String,
}

impl VerifyIssue {
    fn new(line: usize, class: &str, message: &str) -> Self {
        VerifyIssue {
            line,
            class: class.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub struct VerifyReport {
    pub events: usize,
    pub issues: Vec<VerifyIssue>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn draft(event_type: &str, body: Value) -> Draft {
        Draft {
            ts: None,
            actor: "test".into(),
            event_type: event_type.into(),
            body,
            evidence: vec![],
            op: None,
        }
    }

    fn temp_ledger() -> (tempdir::TempDir, Ledger) {
        let dir = tempdir::TempDir::new("banto").unwrap();
        let ledger = Ledger::create(&dir.path().join("ledger.jsonl")).unwrap();
        (dir, ledger)
    }

    // tempdir クレートを増やさないための最小テンポラリディレクトリ。
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        pub struct TempDir(PathBuf);
        impl TempDir {
            pub fn new(prefix: &str) -> std::io::Result<TempDir> {
                let n = N.fetch_add(1, Ordering::SeqCst);
                let path =
                    std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
                std::fs::create_dir_all(&path)?;
                Ok(TempDir(path))
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn append_builds_a_valid_chain() {
        let (_dir, ledger) = temp_ledger();
        let a = ledger
            .append(draft("note.capture", json!({"text": "a"})))
            .unwrap();
        let b = ledger
            .append(draft("note.capture", json!({"text": "b"})))
            .unwrap();
        assert_eq!(a.seq, 0);
        assert_eq!(a.prev, None);
        assert_eq!(b.seq, 1);
        assert_eq!(b.prev, Some(a.id.clone()));
        let report = ledger.verify(None, false).unwrap();
        assert!(report.ok(), "{:?}", report.issues);
        assert_eq!(report.events, 2);
    }

    #[test]
    fn tampering_is_detected() {
        let (_dir, ledger) = temp_ledger();
        ledger
            .append(draft("note.capture", json!({"text": "honest"})))
            .unwrap();
        ledger
            .append(draft("note.capture", json!({"text": "second"})))
            .unwrap();
        let raw = std::fs::read_to_string(ledger.path()).unwrap();
        let tampered = raw.replace("honest", "hacked!");
        assert_ne!(raw, tampered);
        std::fs::write(ledger.path(), tampered).unwrap();
        let report = ledger.verify(None, false).unwrap();
        assert!(!report.ok());
        assert!(report.issues.iter().any(|i| i.class == "validation"));
    }

    #[test]
    fn non_canonical_line_is_detected() {
        let (_dir, ledger) = temp_ledger();
        ledger
            .append(draft("note.capture", json!({"text": "x"})))
            .unwrap();
        let raw = std::fs::read_to_string(ledger.path()).unwrap();
        // 意味は同じだが空白を足す: id は一致するが正準形検査で落ちるべき。
        let padded = raw.trim_end().replace("\":", "\": ") + "\n";
        std::fs::write(ledger.path(), padded).unwrap();
        let report = ledger.verify(None, false).unwrap();
        assert!(report.issues.iter().any(|i| i.class == "non-canonical"));
    }

    #[test]
    fn op_is_idempotent_and_conflicts_on_content_change() {
        let (_dir, ledger) = temp_ledger();
        let mut d = draft("loop.touch", json!({"slug": "x"}));
        d.op = Some("touch-x-1".into());
        let first = ledger.append(d.clone()).unwrap();
        let second = ledger.append(d.clone()).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(ledger.events().unwrap().len(), 1);
        d.body = json!({"slug": "y"});
        assert!(matches!(ledger.append(d), Err(KernelError::OpConflict(_))));
    }

    #[test]
    fn append_recovers_from_missing_trailing_newline() {
        let (_dir, ledger) = temp_ledger();
        ledger
            .append(draft("note.capture", json!({"text": "first"})))
            .unwrap();
        // エディタや git が末尾改行を剥がした正当なファイルを再現する。
        let raw = std::fs::read_to_string(ledger.path()).unwrap();
        std::fs::write(ledger.path(), raw.trim_end_matches('\n')).unwrap();
        let second = ledger
            .append(draft("note.capture", json!({"text": "second"})))
            .unwrap();
        assert_eq!(second.seq, 1);
        let report = ledger.verify(None, false).unwrap();
        assert!(report.ok(), "{:?}", report.issues);
        assert_eq!(report.events, 2);
    }

    #[test]
    fn deep_verify_checks_evidence_hashes() {
        let (dir, ledger) = temp_ledger();
        let file_path = dir.path().join("evidence.txt");
        std::fs::write(&file_path, b"the primary evidence").unwrap();
        let (sha256, bytes) = crate::hash_file(&file_path).unwrap();
        let mut d = draft("claim.declare", json!({"title": "done"}));
        d.evidence = vec![Evidence {
            sha256,
            uri: Some("evidence.txt".into()),
            media_type: None,
            bytes: Some(bytes),
        }];
        ledger.append(d).unwrap();
        let report = ledger.verify(Some(dir.path()), true).unwrap();
        assert!(report.ok(), "{:?}", report.issues);
        std::fs::write(&file_path, b"evidence, replaced after the claim").unwrap();
        let report = ledger.verify(Some(dir.path()), true).unwrap();
        assert!(report.issues.iter().any(|i| i.class == "evidence-hash"));
    }
}
