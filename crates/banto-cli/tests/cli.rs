//! CLI 結合テスト。docs/cli_spec_v1.md の規則(出力形式・終了コード)を外側から検査する。

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ---- 一意なテンポラリワークスペース ----

static N: AtomicU64 = AtomicU64::new(0);

struct TempWs(PathBuf);

impl TempWs {
    fn new() -> TempWs {
        let n = N.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("banto-cli-test-{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        TempWs(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, content: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }
}

impl Drop for TempWs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn banto(ws: &TempWs) -> Command {
    let mut cmd = Command::cargo_bin("banto").unwrap();
    cmd.current_dir(ws.path());
    cmd
}

fn init_ws() -> TempWs {
    let ws = TempWs::new();
    banto(&ws)
        .args(["init", "--actor", "alice"])
        .assert()
        .success();
    ws
}

fn ledger_path(ws: &TempWs) -> PathBuf {
    ws.path().join("ledger/ledger.jsonl")
}

/// `✓ 記録 ... id=<hex64>` の出力から id を抜く。
fn extract_id(output: &std::process::Output) -> String {
    let s = String::from_utf8_lossy(&output.stdout);
    let i = s.find("id=").expect("id= in output");
    s[i + 3..i + 67].to_string()
}

fn stdout_json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout is JSON")
}

// ---- init と発見 ----

#[test]
fn init_creates_workspace_and_refuses_twice() {
    let ws = TempWs::new();
    banto(&ws)
        .args(["init", "--actor", "alice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("✓ 初期化"));
    assert!(ws.path().join("banto.toml").is_file());
    assert!(ledger_path(&ws).is_file());
    assert!(ws.path().join("ledger/objects/.gitkeep").is_file());
    // 2回目はエラー(終了コード 1、日本語 stderr)
    banto(&ws)
        .arg("init")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("既にある"));
}

#[test]
fn workspace_is_discovered_from_subdir_and_missing_workspace_errors() {
    let ws = init_ws();
    let sub = ws.path().join("sub/dir");
    std::fs::create_dir_all(&sub).unwrap();
    let mut cmd = Command::cargo_bin("banto").unwrap();
    cmd.current_dir(&sub).arg("brief").assert().success();
    // --root でも指定できる
    let mut cmd = Command::cargo_bin("banto").unwrap();
    cmd.arg("--root")
        .arg(ws.path())
        .arg("brief")
        .assert()
        .success();
    // ワークスペースが無ければ init を案内してエラー
    let empty = TempWs::new();
    banto(&empty)
        .arg("brief")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("banto init"));
}

// ---- ループ ----

#[test]
fn loop_lifecycle_open_next_touch_list_close_reopen() {
    let ws = init_ws();
    banto(&ws)
        .args([
            "loop",
            "open",
            "paper",
            "--title",
            "論文投稿",
            "--next",
            "著者名を入れる",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("✓ 記録 seq=0 type=loop.open"));
    // 稼働中の slug は開けない
    banto(&ws)
        .args(["loop", "open", "paper", "--title", "重複"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("稼働中"));
    banto(&ws)
        .args(["loop", "next", "paper", "図1を差し替える"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type=loop.next"));
    banto(&ws)
        .args(["loop", "touch", "paper", "--note", "少し進めた"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type=loop.touch"));
    // 存在しない slug はエラー
    banto(&ws)
        .args(["loop", "touch", "ghost"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("存在しない"));
    banto(&ws)
        .args(["loop", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("paper").and(predicate::str::contains("論文投稿")));
    banto(&ws)
        .args(["loop", "close", "paper", "--outcome", "提出した"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type=loop.close"));
    // 閉じた後の再開は --reopen が要る
    banto(&ws)
        .args(["loop", "open", "paper", "--title", "論文投稿"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--reopen"));
    banto(&ws)
        .args(["loop", "open", "paper", "--title", "論文投稿", "--reopen"])
        .assert()
        .success();
    // list は既定 active のみ、--all で全状態
    banto(&ws)
        .args(["loop", "park", "paper"])
        .assert()
        .success();
    banto(&ws)
        .args(["loop", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("paper").not());
    banto(&ws)
        .args(["loop", "list", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[parked]"));
}

#[test]
fn loop_list_json_is_machine_readable() {
    let ws = init_ws();
    banto(&ws)
        .args([
            "loop",
            "open",
            "paper",
            "--title",
            "論文投稿",
            "--kind",
            "project",
            "--next",
            "著者名",
        ])
        .assert()
        .success();
    let out = banto(&ws)
        .args(["loop", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let v = stdout_json(&out);
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["slug"], "paper");
    assert_eq!(arr[0]["title"], "論文投稿");
    assert_eq!(arr[0]["kind"], "project");
    assert_eq!(arr[0]["status"], "active");
    assert_eq!(arr[0]["next"], "著者名");
}

// ---- 捕捉(video / transcript / note) ----

#[test]
fn video_logs_file_as_evidence_with_relative_uri() {
    let ws = init_ws();
    ws.write("v.mp4", b"not really a video, but bytes");
    banto(&ws)
        .args(["video", "v.mp4", "--note", "朝の統括"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type=video.log"));
    let out = banto(&ws)
        .args(["log", "--type", "video.log", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let v = stdout_json(&out);
    let ev = &v[0]["evidence"][0];
    assert_eq!(ev["uri"], "v.mp4");
    assert_eq!(ev["media_type"], "video/mp4");
    assert_eq!(ev["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(v[0]["body"]["filename"], "v.mp4");
    assert_eq!(v[0]["body"]["note"], "朝の統括");
}

#[test]
fn transcript_requires_video_log_event() {
    let ws = init_ws();
    ws.write("v.mp4", b"video bytes");
    let out = banto(&ws)
        .args(["video", "v.mp4"])
        .assert()
        .success()
        .get_output()
        .clone();
    let video_id = extract_id(&out);
    banto(&ws)
        .args(["transcript", &video_id, "--text", "おはようございます"])
        .assert()
        .success()
        .stdout(predicate::str::contains("type=video.transcript"));
    // video.log 以外の id はエラー
    let out = banto(&ws)
        .args(["note", "ただのメモ"])
        .assert()
        .success()
        .get_output()
        .clone();
    let note_id = extract_id(&out);
    banto(&ws)
        .args(["transcript", &note_id, "--text", "x"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("video.log ではない"));
    // --file / --text のどちらも無いのは使い方エラー(clap 既定の 2)
    banto(&ws).args(["transcript", &video_id]).assert().code(2);
}

// ---- 導出ビュー ----

#[test]
fn brief_shows_loop_and_stall_marker() {
    let ws = init_ws();
    banto(&ws)
        .args([
            "loop",
            "open",
            "paper",
            "--title",
            "論文投稿",
            "--next",
            "著者名を入れる",
        ])
        .assert()
        .success();
    banto(&ws).arg("brief").assert().success().stdout(
        predicate::str::contains("paper")
            .and(predicate::str::contains("論文投稿"))
            .and(predicate::str::contains("次の一手: 著者名を入れる"))
            .and(predicate::str::contains("保留場: 0 件"))
            .and(predicate::str::contains("⚠").not()),
    );
    let out = banto(&ws)
        .args(["brief", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let v = stdout_json(&out);
    assert_eq!(v["active"][0]["slug"], "paper");
    assert_eq!(v["active"][0]["stalled"], false);
    assert_eq!(v["wip_exceeded"], false);
    // 停滞閾値を 0 日にすれば即 ⚠(放置 0 日 >= 0)
    let toml_path = ws.path().join("banto.toml");
    let mut cfg = std::fs::read_to_string(&toml_path).unwrap();
    cfg.push_str("\nstall_days = 0\n");
    std::fs::write(&toml_path, cfg).unwrap();
    banto(&ws)
        .arg("brief")
        .assert()
        .success()
        .stdout(predicate::str::contains("⚠"));
}

#[test]
fn health_rings_bells_with_exit_code_1() {
    let ws = init_ws();
    // 空の台帳: 朝の統括が一度も無い → 鐘 → 終了コード 1
    banto(&ws)
        .arg("health")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("朝の統括"));
    ws.write("v.mp4", b"video bytes");
    banto(&ws).args(["video", "v.mp4"]).assert().success();
    let out = banto(&ws)
        .args(["health", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let v = stdout_json(&out);
    assert_eq!(v["total_events"], 1);
    assert_eq!(v["bells"].as_array().unwrap().len(), 0);
    // WIP 超過(project 3 本 > 上限 2)で再び鐘
    for slug in ["a", "b", "c"] {
        banto(&ws)
            .args(["loop", "open", slug, "--title", slug])
            .assert()
            .success();
    }
    banto(&ws)
        .arg("health")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("稼働中プロジェクトが 3 本"));
}

#[test]
fn verify_passes_then_detects_tampering() {
    let ws = init_ws();
    banto(&ws).args(["note", "honest note"]).assert().success();
    banto(&ws).args(["note", "second note"]).assert().success();
    banto(&ws)
        .args(["verify", "--evidence", "--deep"])
        .assert()
        .success()
        .stdout(predicate::str::contains("✓ 台帳は健全"));
    // 台帳の行を書き換えると verify が落ちる(終了コード 1)
    let path = ledger_path(&ws);
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, raw.replace("honest", "hacked")).unwrap();
    banto(&ws)
        .arg("verify")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("✗ 問題"));
}

// ---- ゲート ----

#[test]
fn gate_declare_requires_evidence() {
    let ws = init_ws();
    banto(&ws)
        .args(["gate", "declare", "実験おわった"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("証拠").and(predicate::str::contains("不変量2")));
}

#[test]
fn gate_declare_with_evidence_notes_confidence_is_not_judgment() {
    let ws = init_ws();
    ws.write("result.txt", "p < 0.05".as_bytes());
    banto(&ws)
        .args([
            "gate",
            "declare",
            "実験完了",
            "--evidence",
            "result.txt",
            "--confidence",
            "0.9",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("type=claim.declare")
                .and(predicate::str::contains("判定には使われない")),
        );
}

#[test]
fn gate_verify_rejects_same_actor_and_accepts_independent_actor() {
    let ws = init_ws();
    ws.write("result.txt", b"the artifact");
    let out = banto(&ws)
        .args(["gate", "declare", "成果物完成", "--evidence", "result.txt"])
        .assert()
        .success()
        .get_output()
        .clone();
    let claim_id = extract_id(&out);
    // 宣言と同じ actor(既定 alice)は検証できない
    banto(&ws)
        .args(["gate", "verify", &claim_id, "--verdict", "pass"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("同じ actor").and(predicate::str::contains("不変量3")));
    // 独立した actor なら通る
    banto(&ws)
        .args([
            "gate",
            "verify",
            &claim_id,
            "--verdict",
            "pass",
            "--actor",
            "agent:claude",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("type=claim.verify"));
    // check は最新判定を示す
    banto(&ws)
        .args(["gate", "check", &claim_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("成立(pass)"));
}

#[test]
fn gate_check_exit_codes_follow_evidence_resolution() {
    let ws = init_ws();
    let file = ws.write("result.txt", b"original evidence");
    let out = banto(&ws)
        .args(["gate", "declare", "できた", "--evidence", "result.txt"])
        .assert()
        .success()
        .get_output()
        .clone();
    let claim_id = extract_id(&out);
    banto(&ws)
        .args(["gate", "check", &claim_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("✓ 証拠はすべて解決"));
    // 中身を差し替えるとハッシュ不一致 → 終了コード 1
    std::fs::write(&file, b"evidence replaced after the claim").unwrap();
    banto(&ws)
        .args(["gate", "check", &claim_id])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("ハッシュ不一致"));
    // 消すとファイル欠落 → 終了コード 1
    std::fs::remove_file(&file).unwrap();
    banto(&ws)
        .args(["gate", "check", &claim_id])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("ファイルが見つからない"));
}

// ---- 汎用追記と冪等キー ----

#[test]
fn append_op_is_idempotent_and_conflicts_on_content_change() {
    let ws = init_ws();
    let first = banto(&ws)
        .args([
            "append",
            "note.capture",
            "--body",
            r#"{"text":"x"}"#,
            "--op",
            "once-1",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let second = banto(&ws)
        .args([
            "append",
            "note.capture",
            "--body",
            r#"{"text":"x"}"#,
            "--op",
            "once-1",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(extract_id(&first), extract_id(&second));
    let out = banto(&ws)
        .args(["log", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(stdout_json(&out).as_array().unwrap().len(), 1);
    // 同じ op で中身が違えば衝突エラー
    banto(&ws)
        .args([
            "append",
            "note.capture",
            "--body",
            r#"{"text":"y"}"#,
            "--op",
            "once-1",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("追記に失敗"));
}

#[test]
fn append_store_copies_evidence_into_objects() {
    let ws = init_ws();
    ws.write("data.json", br#"{"k":1}"#);
    banto(&ws)
        .args([
            "append",
            "claim.declare",
            "--body",
            r#"{"title":"t"}"#,
            "--evidence",
            "data.json",
            "--store",
        ])
        .assert()
        .success();
    let out = banto(&ws)
        .args(["log", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let v = stdout_json(&out);
    let ev = &v[0]["evidence"][0];
    let uri = ev["uri"].as_str().unwrap();
    let sha = ev["sha256"].as_str().unwrap();
    assert_eq!(uri, format!("ledger/objects/{sha}"));
    assert!(ws.path().join(uri).is_file());
    assert_eq!(ev["media_type"], "application/json");
    // ワークスペース外のファイルは絶対パスの uri になる
    let outside = TempWs::new();
    let ext = outside.write("ext.txt", b"outside evidence");
    banto(&ws)
        .args(["append", "note.capture"])
        .arg("--evidence")
        .arg(&ext)
        .assert()
        .success();
    let out = banto(&ws)
        .args(["log", "--type", "note.capture", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let v = stdout_json(&out);
    let uri = v[0]["evidence"][0]["uri"].as_str().unwrap();
    assert!(uri.starts_with('/'), "uri should be absolute: {uri}");
    // deep verify は store 済み・外部証拠ともに通る
    banto(&ws).args(["verify", "--deep"]).assert().success();
}

// ---- レビューで確定した欠陥の回帰テスト ----

#[test]
fn gate_check_excludes_same_actor_verify_from_judgment() {
    let ws = init_ws();
    ws.write("out.txt", b"artifact");
    let declare = banto(&ws)
        .args(["gate", "declare", "完了主張", "--evidence", "out.txt"])
        .output()
        .unwrap();
    let claim_id = extract_id(&declare);
    // 汎用 append で自分自身の claim.verify を書き込む(不変量3の迂回攻撃)。
    let body = format!(r#"{{"claim":"{claim_id}","verdict":"pass"}}"#);
    banto(&ws)
        .args(["append", "claim.verify", "--body", &body])
        .assert()
        .success();
    // gate check は自己検証を判定から除外し、未検証のままと報告する。
    banto(&ws)
        .args(["gate", "check", &claim_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("不変量3 により判定から除外"))
        .stdout(predicate::str::contains("未検証"))
        .stdout(predicate::str::contains("成立(pass)").not());
}

#[test]
fn gate_check_flags_zero_evidence_claim_with_exit_1() {
    let ws = init_ws();
    // 汎用 append なら証拠なしの claim.declare も書ける(封筒としては正当)。
    let declare = banto(&ws)
        .args([
            "append",
            "claim.declare",
            "--body",
            r#"{"title":"証拠なし"}"#,
        ])
        .output()
        .unwrap();
    let claim_id = extract_id(&declare);
    // しかし読むときに厳格: gate check は不変量2違反として終了コード1。
    banto(&ws)
        .args(["gate", "check", &claim_id])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("不変量2"));
}

#[test]
fn append_survives_stripped_trailing_newline() {
    let ws = init_ws();
    banto(&ws).args(["note", "first"]).assert().success();
    // 末尾改行を剥がす(エディタ・git・中断で起きる正当なファイル状態)。
    let ledger = ledger_path(&ws);
    let raw = std::fs::read_to_string(&ledger).unwrap();
    std::fs::write(&ledger, raw.trim_end_matches('\n')).unwrap();
    banto(&ws).args(["note", "second"]).assert().success();
    banto(&ws).args(["verify"]).assert().success();
    let log = banto(&ws).args(["log", "--json"]).output().unwrap();
    let events = stdout_json(&log);
    assert_eq!(events.as_array().unwrap().len(), 2);
}

#[test]
fn video_sha256_records_remote_original_without_file() {
    let ws = init_ws();
    let hash = "a".repeat(64);
    banto(&ws)
        .args([
            "video",
            "--sha256",
            &hash,
            "--filename",
            "asa_2026-07-18.mov",
            "--bytes",
            "31457280",
            "--note",
            "端末に原本",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("video.log"))
        .stdout(predicate::str::contains("原本は端末側"));
    banto(&ws).args(["verify", "--deep"]).assert().success(); // uri なしは deep でも解決対象外
                                                              // 不正な hex は拒否
    banto(&ws)
        .args(["video", "--sha256", "xyz", "--filename", "a.mov"])
        .assert()
        .failure();
}

#[test]
fn journal_is_medium_agnostic_and_idempotent() {
    let ws = init_ws();
    // テキストだけで記帳できる(媒体非依存)
    let out = banto(&ws)
        .args([
            "journal",
            "--text",
            "今日は倉庫を片付けた。棚は明日組む。",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let id1 = extract_id(&out);
    // 同じテキストの再記帳は同一イベント(冪等)
    let out2 = banto(&ws)
        .args([
            "journal",
            "--text",
            "今日は倉庫を片付けた。棚は明日組む。",
        ])
        .output()
        .unwrap();
    assert_eq!(id1, extract_id(&out2));
    // journal.entry が朝の統括の鐘を止める
    banto(&ws).args(["health"]).assert().success();
    // 原本ハッシュ付きも通る
    banto(&ws)
        .args([
            "journal",
            "--text",
            "別の日の統括",
            "--sha256",
            &"b".repeat(64),
            "--filename",
            "asa.m4a",
        ])
        .assert()
        .success();
    banto(&ws).args(["verify", "--deep"]).assert().success();
}

#[test]
fn video_recording_same_file_twice_is_idempotent() {
    let ws = init_ws();
    ws.write("v.mp4", b"same video bytes");
    let a = banto(&ws).args(["video", "v.mp4"]).output().unwrap();
    let b = banto(&ws).args(["video", "v.mp4"]).output().unwrap();
    assert_eq!(extract_id(&a), extract_id(&b));
    let log = banto(&ws).args(["log", "--json"]).output().unwrap();
    assert_eq!(stdout_json(&log).as_array().unwrap().len(), 1);
}
