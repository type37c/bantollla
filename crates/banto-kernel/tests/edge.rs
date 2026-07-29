//! 敵対的エッジテスト: 公開 API だけを通して台帳の防御を叩く。
//!
//! 各テストは自前の一意なテンポラリディレクトリを作り、Drop で消す
//! (カーネル内部テストと同じ流儀)。判定は VerifyIssue の class 名で断定する。

use banto_kernel::{hash_bytes, Draft, Envelope, Evidence, Ledger, VerifyReport, SCHEMA};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let n = N.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("banto-edge-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_ledger() -> (TempDir, Ledger) {
    let dir = TempDir::new();
    let ledger = Ledger::create(&dir.path().join("ledger.jsonl")).unwrap();
    (dir, ledger)
}

fn draft(actor: &str, event_type: &str, body: Value) -> Draft {
    Draft {
        ts: None,
        actor: actor.into(),
        event_type: event_type.into(),
        body,
        evidence: vec![],
        op: None,
    }
}

fn classes(report: &VerifyReport) -> Vec<&str> {
    report.issues.iter().map(|i| i.class.as_str()).collect()
}

fn read_lines(ledger: &Ledger) -> Vec<String> {
    std::fs::read_to_string(ledger.path())
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn write_lines(ledger: &Ledger, lines: &[String]) {
    let mut out = lines.join("\n");
    out.push('\n');
    std::fs::write(ledger.path(), out).unwrap();
}

#[test]
fn unicode_emoji_japanese_roundtrip_and_verify() {
    let (_dir, ledger) = temp_ledger();
    let body = json!({
        "text": "番頭は黙って死なせない 🏮📜",
        "かぎ": "キーも日本語",
        "mixed": "ASCII と 日本語 と émoji 🎌😀",
        // 結合濁点 (か + U+3099): 正規化されずバイト列のまま残るべき。
        "combining": "か\u{3099}き\u{3099}",
    });
    let actor = "主人:花子 🐢";
    let appended = ledger
        .append(draft(actor, "note.capture", body.clone()))
        .unwrap();
    let events = ledger.events().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], appended);
    assert_eq!(events[0].body, body);
    assert_eq!(events[0].actor, actor);
    let report = ledger.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(report.events, 1);
}

#[test]
fn deeply_nested_body_roundtrip_is_canonical_and_stable() {
    let (_dir, ledger) = temp_ledger();
    let mut body = json!({"z": 0, "a": [1, {"b": [true, null, {"深い": "谷"}]}]});
    for _ in 0..20 {
        body = json!({"outer": [body, {"倉": "庫"}], "先": "後"});
    }
    ledger
        .append(draft("tester", "note.capture", body.clone()))
        .unwrap();
    let line = read_lines(&ledger)[0].clone();
    let events = ledger.events().unwrap();
    assert_eq!(events[0].body, body);
    // 正準形はバイト単位で安定: パースした封筒を再直列化しても同じ行になる。
    assert_eq!(events[0].to_line().unwrap(), line);
    let report = ledger.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
}

#[test]
fn float_body_survives_roundtrip_and_verify() {
    let (_dir, ledger) = temp_ledger();
    let body = json!({"x": 1.0, "y": 0.5, "big": 1e300, "neg": -2.25});
    ledger
        .append(draft("tester", "note.capture", body.clone()))
        .unwrap();
    let events = ledger.events().unwrap();
    assert_eq!(events[0].body, body);
    // 浮動小数の正準表記は固定: 1.0 は "1.0" のまま台帳に書かれる。
    let line = read_lines(&ledger)[0].clone();
    assert!(line.contains("\"x\":1.0"), "{line}");
    assert_eq!(events[0].to_line().unwrap(), line);
    let report = ledger.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
}

#[test]
fn key_order_attack_is_flagged_non_canonical() {
    let (_dir, ledger) = temp_ledger();
    ledger
        .append(draft("tester", "note.capture", json!({"text": "one"})))
        .unwrap();
    ledger
        .append(draft("tester", "note.capture", json!({"text": "two"})))
        .unwrap();
    let mut lines = read_lines(&ledger);
    let last = lines.pop().unwrap();
    // 同じ内容・同じ id のまま、先頭の actor キーを末尾へ移してキー順だけ変える。
    let prefix = "{\"actor\":\"tester\",";
    assert!(last.starts_with(prefix), "{last}");
    let rest = &last[prefix.len()..last.len() - 1];
    lines.push(format!("{{{rest},\"actor\":\"tester\"}}"));
    write_lines(&ledger, &lines);
    let report = ledger.verify(None, false).unwrap();
    // id は一致するので validation は通り、正準形検査だけが落ちる。
    assert_eq!(classes(&report), ["non-canonical"], "{:?}", report.issues);
    assert_eq!(report.events, 2);
}

#[test]
fn deletion_attack_is_flagged() {
    let (_dir, ledger) = temp_ledger();
    for i in 0..3 {
        ledger
            .append(draft("tester", "note.capture", json!({"n": i})))
            .unwrap();
    }
    let mut lines = read_lines(&ledger);
    lines.remove(1); // 真ん中 (seq 1) を消す
    write_lines(&ledger, &lines);
    let report = ledger.verify(None, false).unwrap();
    assert!(!report.ok());
    let cs = classes(&report);
    assert!(cs.contains(&"seq"), "{cs:?}");
    assert!(cs.contains(&"chain"), "{cs:?}");
}

#[test]
fn duplication_attack_is_flagged() {
    let (_dir, ledger) = temp_ledger();
    ledger
        .append(draft("tester", "note.capture", json!({"n": 0})))
        .unwrap();
    ledger
        .append(draft("tester", "note.capture", json!({"n": 1})))
        .unwrap();
    let mut lines = read_lines(&ledger);
    lines.push(lines.last().unwrap().clone());
    write_lines(&ledger, &lines);
    let report = ledger.verify(None, false).unwrap();
    assert!(!report.ok());
    let cs = classes(&report);
    // 複製行は seq が進まず、prev も直前の id と食い違う。
    assert!(cs.contains(&"seq"), "{cs:?}");
    assert!(cs.contains(&"chain"), "{cs:?}");
}

#[test]
fn truncation_attack_is_flagged_as_parse() {
    let (_dir, ledger) = temp_ledger();
    ledger
        .append(draft("tester", "note.capture", json!({"text": "kept"})))
        .unwrap();
    ledger
        .append(draft("tester", "note.capture", json!({"text": "cut"})))
        .unwrap();
    let lines = read_lines(&ledger);
    let last = &lines[1];
    // 最終行を半分で切る (本文は ASCII なので UTF-8 境界は安全)。
    let truncated = format!("{}\n{}", lines[0], &last[..last.len() / 2]);
    std::fs::write(ledger.path(), truncated).unwrap();
    let report = ledger.verify(None, false).unwrap();
    assert!(classes(&report).contains(&"parse"), "{:?}", report.issues);
    assert_eq!(report.events, 1);
}

#[test]
fn two_ledger_handles_on_same_path_chain_correctly() {
    let dir = TempDir::new();
    let path = dir.path().join("ledger.jsonl");
    let a = Ledger::create(&path).unwrap();
    let b = Ledger::open(&path).unwrap();
    let e1 = a
        .append(draft("actor:a", "note.capture", json!({"via": "a"})))
        .unwrap();
    let e2 = b
        .append(draft("actor:b", "note.capture", json!({"via": "b"})))
        .unwrap();
    let e3 = a
        .append(draft("actor:a", "note.capture", json!({"via": "a2"})))
        .unwrap();
    assert_eq!(e2.seq, 1);
    assert_eq!(e2.prev, Some(e1.id.clone()));
    assert_eq!(e3.seq, 2);
    assert_eq!(e3.prev, Some(e2.id.clone()));
    let report = b.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(report.events, 3);
}

#[test]
fn wrong_evidence_hash_is_flagged_by_deep_verify() {
    let (dir, ledger) = temp_ledger();
    std::fs::write(dir.path().join("成果物.txt"), "実際の中身".as_bytes()).unwrap();
    let mut d = draft("tester", "claim.declare", json!({"title": "done"}));
    d.evidence = vec![Evidence {
        // 実ファイルとは別の中身のハッシュを記録しておく。
        sha256: hash_bytes("台帳に記録された別の中身".as_bytes()),
        uri: Some("成果物.txt".into()),
        media_type: Some("text/plain".into()),
        bytes: None,
    }];
    ledger.append(d).unwrap();
    // 浅い検査はファイルの存在しか見ないので通る。
    let shallow = ledger.verify(Some(dir.path()), false).unwrap();
    assert!(shallow.ok(), "{:?}", shallow.issues);
    // 深い検査はハッシュ不一致を掴む。
    let deep = ledger.verify(Some(dir.path()), true).unwrap();
    assert_eq!(classes(&deep), ["evidence-hash"], "{:?}", deep.issues);
}

#[test]
fn empty_ledger_is_valid_with_zero_events() {
    let (_dir, ledger) = temp_ledger();
    assert!(ledger.events().unwrap().is_empty());
    let report = ledger.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(report.events, 0);
}

#[test]
fn kernel_append_chains_after_manual_but_valid_external_append() {
    let (_dir, ledger) = temp_ledger();
    let first = ledger
        .append(draft("tester", "note.capture", json!({"n": 0})))
        .unwrap();
    // カーネルの封筒 API で正しい行を作り、Ledger::append を通さず手で追記する。
    let mut env = Envelope {
        schema: SCHEMA.into(),
        seq: first.seq + 1,
        prev: Some(first.id.clone()),
        ts: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        actor: "external:script".into(),
        event_type: "note.capture".into(),
        body: json!({"n": 1, "手書き": true}),
        evidence: vec![],
        op: None,
        id: String::new(),
    };
    env.id = env.compute_id().unwrap();
    env.validate().unwrap();
    let mut raw = std::fs::read_to_string(ledger.path()).unwrap();
    raw.push_str(&env.to_line().unwrap());
    raw.push('\n');
    std::fs::write(ledger.path(), raw).unwrap();
    // カーネル経由の次の追記が、外部行の上に正しく連鎖する。
    let third = ledger
        .append(draft("tester", "note.capture", json!({"n": 2})))
        .unwrap();
    assert_eq!(third.seq, 2);
    assert_eq!(third.prev, Some(env.id.clone()));
    let report = ledger.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(report.events, 3);
}

#[test]
fn unreadable_deep_body_is_refused_before_writing() {
    let (_dir, ledger) = temp_ledger();
    // serde_json の再帰上限(128)を超える深さ: 書けても二度と読めない行になる。
    let mut body = json!(0);
    for _ in 0..200 {
        body = json!([body]);
    }
    let err = ledger.append(draft("tester", "note.capture", body));
    assert!(err.is_err(), "deep body must be refused");
    // 台帳には何も書かれておらず、以後も普通に使える(自壊しない)。
    assert_eq!(ledger.events().unwrap().len(), 0);
    ledger
        .append(draft(
            "tester",
            "note.capture",
            json!({"text": "still alive"}),
        ))
        .unwrap();
    let report = ledger.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
}

#[test]
fn crlf_converted_ledger_is_flagged() {
    let (_dir, ledger) = temp_ledger();
    ledger
        .append(draft("tester", "note.capture", json!({"text": "lf"})))
        .unwrap();
    let raw = std::fs::read_to_string(ledger.path()).unwrap();
    std::fs::write(ledger.path(), raw.replace('\n', "\r\n")).unwrap();
    let report = ledger.verify(None, false).unwrap();
    assert!(
        classes(&report).contains(&"non-canonical"),
        "{:?}",
        report.issues
    );
}

#[test]
fn evidence_uri_cannot_escape_evidence_root() {
    let (dir, ledger) = temp_ledger();
    // evidence root の外に「秘密」を置き、../ で指す証拠を積む。
    let root = dir.path().join("objects");
    std::fs::create_dir_all(&root).unwrap();
    let secret = b"outside the evidence root";
    std::fs::write(dir.path().join("secret.txt"), secret).unwrap();
    let mut d = draft("tester", "claim.declare", json!({"title": "sneaky"}));
    d.evidence = vec![Evidence {
        sha256: hash_bytes(secret),
        uri: Some("../secret.txt".into()),
        media_type: None,
        bytes: Some(secret.len() as u64),
    }];
    ledger.append(d).unwrap();
    let report = ledger.verify(Some(&root), true).unwrap();
    assert!(
        classes(&report).contains(&"evidence-invalid"),
        "{:?}",
        report.issues
    );
    // ハッシュ照合(オラクル)まで進んでいないこと。
    assert!(!classes(&report).contains(&"evidence-hash"));
}

#[test]
fn missing_evidence_file_is_flagged() {
    let (dir, ledger) = temp_ledger();
    let mut d = draft("tester", "claim.declare", json!({"title": "no file"}));
    d.evidence = vec![Evidence {
        sha256: hash_bytes(b"whatever"),
        uri: Some("gone.txt".into()),
        media_type: None,
        bytes: None,
    }];
    ledger.append(d).unwrap();
    let report = ledger.verify(Some(dir.path()), false).unwrap();
    assert!(
        classes(&report).contains(&"evidence-missing"),
        "{:?}",
        report.issues
    );
}

#[test]
fn duplicate_op_in_ledger_is_flagged() {
    let (_dir, ledger) = temp_ledger();
    let first = ledger
        .append({
            let mut d = draft("tester", "loop.touch", json!({"slug": "x"}));
            d.op = Some("touch-1".into());
            d
        })
        .unwrap();
    // 同じ op を持つ2行目を手書きで正しく連鎖させる(appendは冪等で拒むため)。
    let mut env = Envelope {
        schema: SCHEMA.into(),
        seq: 1,
        prev: Some(first.id.clone()),
        ts: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        actor: "external:script".into(),
        event_type: "loop.touch".into(),
        body: json!({"slug": "y"}),
        evidence: vec![],
        op: Some("touch-1".into()),
        id: String::new(),
    };
    env.id = env.compute_id().unwrap();
    let mut lines = read_lines(&ledger);
    lines.push(env.to_line().unwrap());
    write_lines(&ledger, &lines);
    let report = ledger.verify(None, false).unwrap();
    assert!(
        classes(&report).contains(&"op-duplicate"),
        "{:?}",
        report.issues
    );
}

#[test]
fn interior_empty_line_is_flagged_but_trailing_newline_is_not() {
    let (_dir, ledger) = temp_ledger();
    ledger
        .append(draft("tester", "note.capture", json!({"n": 1})))
        .unwrap();
    ledger
        .append(draft("tester", "note.capture", json!({"n": 2})))
        .unwrap();
    // 正常な台帳(末尾改行あり)は empty-line を出さない。
    let report = ledger.verify(None, false).unwrap();
    assert!(report.ok(), "{:?}", report.issues);
    // 行間に空行を差し込むと鳴る。
    let lines = read_lines(&ledger);
    let with_gap = format!("{}\n\n{}\n", lines[0], lines[1]);
    std::fs::write(ledger.path(), with_gap).unwrap();
    let report = ledger.verify(None, false).unwrap();
    assert!(
        classes(&report).contains(&"empty-line"),
        "{:?}",
        report.issues
    );
}
