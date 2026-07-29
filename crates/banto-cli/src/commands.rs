//! 各コマンドの実装。文言と終了コードの規則は docs/cli_spec_v1.md に従う。

use crate::config::{Workspace, R};
use crate::render;
use banto_kernel::fold;
use banto_kernel::{Draft, Envelope, Evidence, Ledger, LoopState, LoopStatus};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

// ---- 共通の部品 ----

fn open_ledger(ws: &Workspace) -> R<Ledger> {
    Ledger::open(&ws.ledger_path()).map_err(|e| format!("台帳を開けない: {e}"))
}

fn read_events(ledger: &Ledger) -> R<Vec<Envelope>> {
    ledger.events().map_err(|e| format!("台帳を読めない: {e}"))
}

/// 追記して仕様通りの1行(✓ 記録 ...)を出す。
fn append(
    ws: &Workspace,
    ledger: &Ledger,
    event_type: &str,
    body: Value,
    evidence: Vec<Evidence>,
    op: Option<String>,
) -> R<Envelope> {
    let env = ledger
        .append(Draft {
            ts: None,
            actor: ws.actor.clone(),
            event_type: event_type.into(),
            body,
            evidence,
            op,
        })
        .map_err(|e| format!("追記に失敗: {e}"))?;
    println!(
        "✓ 記録 seq={} type={} id={}",
        env.seq, env.event_type, env.id
    );
    Ok(env)
}

/// body 組み立て(None のキーは省略)。
struct Body(Map<String, Value>);

impl Body {
    fn new() -> Body {
        Body(Map::new())
    }
    fn set(mut self, key: &str, value: impl Into<Value>) -> Body {
        self.0.insert(key.into(), value.into());
        self
    }
    fn set_opt(self, key: &str, value: Option<impl Into<Value>>) -> Body {
        match value {
            Some(v) => self.set(key, v),
            None => self,
        }
    }
    fn into_value(self) -> Value {
        Value::Object(self.0)
    }
}

/// 拡張子ベースの media_type 推定(cli_spec_v1.md の表)。不明は省略。
fn media_type_of(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mt = match ext.as_str() {
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "txt" | "md" => "text/plain",
        "json" => "application/json",
        "jsonl" => "application/jsonl",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return None,
    };
    Some(mt.into())
}

/// ルート内なら相対、外なら絶対の uri。
fn rel_uri(root: &Path, file: &Path) -> R<String> {
    let abs_file = std::fs::canonicalize(file)
        .map_err(|e| format!("証拠ファイルを解決できない: {}: {e}", file.display()))?;
    let abs_root = std::fs::canonicalize(root)
        .map_err(|e| format!("ルートを解決できない: {}: {e}", root.display()))?;
    let path = match abs_file.strip_prefix(&abs_root) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => abs_file,
    };
    Ok(path.to_string_lossy().into_owned())
}

/// ファイルから証拠を作る。--store なら objects/<sha256> へコピーして uri をそこへ向ける。
fn evidence_for(ws: &Workspace, file: &Path, store: bool) -> R<Evidence> {
    if !file.is_file() {
        return Err(format!("証拠ファイルが見つからない: {}", file.display()));
    }
    let (sha256, bytes) = banto_kernel::hash_file(file)
        .map_err(|e| format!("証拠ファイルを読めない: {}: {e}", file.display()))?;
    let uri = if store {
        let objects = ws.objects_path();
        std::fs::create_dir_all(&objects).map_err(|e| format!("objects/ を作れない: {e}"))?;
        let dest = objects.join(&sha256);
        if !dest.is_file() {
            std::fs::copy(file, &dest).map_err(|e| format!("objects/ へコピーできない: {e}"))?;
        }
        format!("{}/{}", ws.objects_rel.trim_end_matches('/'), sha256)
    } else {
        rel_uri(&ws.root, file)?
    };
    Ok(Evidence {
        sha256,
        uri: Some(uri),
        media_type: media_type_of(file),
        bytes: Some(bytes),
    })
}

fn collect_evidence(ws: &Workspace, files: &[PathBuf], store: bool) -> R<Vec<Evidence>> {
    files.iter().map(|f| evidence_for(ws, f, store)).collect()
}

// ---- 初期化・記録 ----

pub fn cmd_init(root: &Path, actor_flag: Option<&str>) -> R<i32> {
    std::fs::create_dir_all(root)
        .map_err(|e| format!("ディレクトリを作れない: {}: {e}", root.display()))?;
    let toml_path = root.join("banto.toml");
    if toml_path.exists() {
        return Err(format!("banto.toml が既にある: {}", toml_path.display()));
    }
    Ledger::create(&root.join(crate::config::DEFAULT_LEDGER))
        .map_err(|e| format!("台帳を作れない: {e}"))?;
    let objects = root.join(crate::config::DEFAULT_OBJECTS);
    std::fs::create_dir_all(&objects).map_err(|e| format!("objects/ を作れない: {e}"))?;
    std::fs::write(objects.join(".gitkeep"), "")
        .map_err(|e| format!(".gitkeep を作れない: {e}"))?;
    let actor = actor_flag
        .map(str::to_owned)
        .unwrap_or_else(crate::config::default_actor);
    let content = format!(
        "# banto ワークスペース設定(すべて省略可。仕様は docs/cli_spec_v1.md)\n\
         actor = {actor:?}\n\
         # stall_days = 4        # project の停滞閾値(日)\n\
         # habit_stall_days = 2  # habit の停滞閾値(日)\n\
         # wip_limit = 2         # 稼働中プロジェクトの上限(habit は数えない)\n\
         \n\
         # [paths]\n\
         # ledger = \"ledger/ledger.jsonl\"\n\
         # objects = \"ledger/objects\"\n"
    );
    std::fs::write(&toml_path, content).map_err(|e| format!("banto.toml を書けない: {e}"))?;
    println!("✓ 初期化 root={} actor={actor}", root.display());
    Ok(0)
}

pub fn cmd_append(
    ws: &Workspace,
    event_type: &str,
    body_str: &str,
    files: &[PathBuf],
    op: Option<String>,
    store: bool,
) -> R<i32> {
    let body: Value =
        serde_json::from_str(body_str).map_err(|e| format!("--body の JSON が不正: {e}"))?;
    if !body.is_object() {
        return Err("--body は JSON オブジェクトでなければならない".into());
    }
    let evidence = collect_evidence(ws, files, store)?;
    let ledger = open_ledger(ws)?;
    append(ws, &ledger, event_type, body, evidence, op)?;
    Ok(0)
}

pub fn cmd_video(
    ws: &Workspace,
    file: &Path,
    note: Option<String>,
    loop_slug: Option<String>,
    store: bool,
) -> R<i32> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("ファイル名を取得できない: {}", file.display()))?
        .to_string();
    let evidence = vec![evidence_for(ws, file, store)?];
    let op = format!("video:{}", evidence[0].sha256);
    let body = Body::new()
        .set("filename", filename)
        .set_opt("note", note)
        .set_opt("loop", loop_slug)
        .into_value();
    let ledger = open_ledger(ws)?;
    append(ws, &ledger, "video.log", body, evidence, Some(op))?;
    Ok(0)
}

/// 朝の統括(journal.entry)。媒体非依存 — テキストが本体で、原本ハッシュは任意。
/// テキストのハッシュを冪等キーにするので、同じ統括の二重記帳は起きない。
#[allow(clippy::too_many_arguments)]
pub fn cmd_journal(
    ws: &Workspace,
    text: Option<String>,
    file: Option<&Path>,
    sha256: Option<String>,
    filename: Option<String>,
    bytes: Option<u64>,
    loop_slug: Option<String>,
) -> R<i32> {
    let text = match (text, file) {
        (Some(t), None) => t,
        (None, Some(path)) => std::fs::read_to_string(path)
            .map_err(|e| format!("ファイルを読めない: {}: {e}", path.display()))?,
        _ => return Err("--text か --file のどちらかを指定する".into()),
    };
    if text.trim().is_empty() {
        return Err("統括のテキストが空".into());
    }
    let text_hash = banto_kernel::hash_bytes(text.as_bytes());
    let mut evidence = vec![Evidence {
        sha256: text_hash.clone(),
        uri: None,
        media_type: Some("text/plain".into()),
        bytes: Some(text.len() as u64),
    }];
    if let Some(orig) = sha256 {
        let orig = orig.trim().to_ascii_lowercase();
        if !banto_kernel::event::is_hex64(&orig) {
            return Err("--sha256 は 64 桁の hex でなければならない".into());
        }
        let name = filename.as_deref().unwrap_or("original");
        evidence.push(Evidence {
            sha256: orig,
            uri: None,
            media_type: media_type_of(Path::new(name)),
            bytes,
        });
    }
    let body = Body::new()
        .set("text", text)
        .set_opt("filename", filename)
        .set_opt("loop", loop_slug)
        .into_value();
    let ledger = open_ledger(ws)?;
    append(
        ws,
        &ledger,
        "journal.entry",
        body,
        evidence,
        Some(format!("journal:{text_hash}")),
    )?;
    Ok(0)
}

/// 原本が手元にないビデオを、撮った端末で計算した SHA-256 だけで記帳する。
/// 契約の本来の形: 動画は端末に置いたまま、台帳はハッシュで参照する(uri なし)。
pub fn cmd_video_remote(
    ws: &Workspace,
    sha256: &str,
    filename: &str,
    bytes: Option<u64>,
    note: Option<String>,
    loop_slug: Option<String>,
) -> R<i32> {
    let sha256 = sha256.trim().to_ascii_lowercase();
    if !banto_kernel::event::is_hex64(&sha256) {
        return Err(format!(
            "--sha256 は 64 桁の hex でなければならない(受け取った長さ: {})。\
             端末では `shasum -a 256 <ファイル>` で計算できる",
            sha256.len()
        ));
    }
    let op = format!("video:{sha256}");
    let evidence = vec![Evidence {
        sha256,
        uri: None,
        media_type: media_type_of(Path::new(filename)),
        bytes,
    }];
    let body = Body::new()
        .set("filename", filename)
        .set_opt("note", note)
        .set_opt("loop", loop_slug)
        .into_value();
    let ledger = open_ledger(ws)?;
    append(ws, &ledger, "video.log", body, evidence, Some(op))?;
    println!("注: 原本は端末側。uri なしの証拠として記帳した(検証はハッシュ照合のみ可能)");
    Ok(0)
}

pub fn cmd_transcript(
    ws: &Workspace,
    event_id: &str,
    file: Option<&Path>,
    text: Option<String>,
) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let video = ledger
        .find(event_id)
        .map_err(|_| format!("イベントが見つからない: {event_id}"))?;
    if video.event_type != "video.log" {
        return Err(format!(
            "{event_id} は video.log ではない(type={})",
            video.event_type
        ));
    }
    let (text, evidence) = match (file, text) {
        (Some(path), None) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("ファイルを読めない: {}: {e}", path.display()))?;
            let ev = evidence_for(ws, path, false)?;
            (content, ev)
        }
        (None, Some(t)) => {
            let ev = Evidence {
                sha256: banto_kernel::hash_bytes(t.as_bytes()),
                uri: None,
                media_type: Some("text/plain".into()),
                bytes: Some(t.len() as u64),
            };
            (t, ev)
        }
        _ => return Err("--file か --text のどちらか一方を指定する".into()),
    };
    let body = Body::new()
        .set("video", event_id)
        .set("text", text)
        .into_value();
    append(ws, &ledger, "video.transcript", body, vec![evidence], None)?;
    Ok(0)
}

pub fn cmd_note(ws: &Workspace, text: String, tags: Vec<String>) -> R<i32> {
    let mut body = Body::new().set("text", text);
    if !tags.is_empty() {
        body = body.set(
            "tags",
            Value::Array(tags.into_iter().map(Value::String).collect()),
        );
    }
    let ledger = open_ledger(ws)?;
    append(ws, &ledger, "note.capture", body.into_value(), vec![], None)?;
    Ok(0)
}

// ---- ループ ----

pub fn cmd_loop_open(
    ws: &Workspace,
    slug: String,
    title: String,
    kind: Option<String>,
    next: Option<String>,
    reopen: bool,
) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    if let Some(state) = fold::fold_loops(&events).get(&slug) {
        match state.status {
            LoopStatus::Active => {
                return Err(format!("ループ '{slug}' は既に稼働中"));
            }
            _ if !reopen => {
                return Err(format!(
                    "ループ '{slug}' は {} 状態。開き直すには --reopen を付ける",
                    state.status.label()
                ));
            }
            _ => {}
        }
    }
    let body = Body::new()
        .set("slug", slug)
        .set("title", title)
        .set_opt("kind", kind)
        .set_opt("next", next)
        .into_value();
    append(ws, &ledger, "loop.open", body, vec![], None)?;
    Ok(0)
}

/// next/touch/close/park/kill の共通形。存在しない slug はエラー。
pub fn cmd_loop_verb(
    ws: &Workspace,
    verb: &str,
    slug: String,
    extra_key: &str,
    extra: Option<String>,
) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    if !fold::fold_loops(&events).contains_key(&slug) {
        return Err(format!(
            "ループ '{slug}' は存在しない(`banto loop open` で開く)"
        ));
    }
    let body = Body::new()
        .set("slug", slug)
        .set_opt(extra_key, extra)
        .into_value();
    append(ws, &ledger, &format!("loop.{verb}"), body, vec![], None)?;
    Ok(0)
}

pub fn cmd_loop_list(ws: &Workspace, all: bool, json: bool) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    let loops = fold::fold_loops(&events);
    let selected: Vec<&LoopState> = loops
        .values()
        .filter(|l| all || l.status == LoopStatus::Active)
        .collect();
    if json {
        let arr = Value::Array(selected.iter().map(|l| render::loop_value(l)).collect());
        println!("{arr}");
    } else if selected.is_empty() {
        println!("ループはない。");
    } else {
        for l in &selected {
            println!("{}", render::loop_line(l));
        }
    }
    Ok(0)
}

// ---- 導出ビュー ----

pub fn cmd_brief(ws: &Workspace, json: bool) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    let cfg = ws.brief_config();
    let b = fold::brief(&events, &cfg);
    if json {
        println!("{}", render::brief_value(&b, &cfg));
    } else {
        print!("{}", render::brief_text(&b, &cfg));
    }
    Ok(0)
}

pub fn cmd_health(ws: &Workspace, json: bool) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    let h = fold::health(&events, &ws.brief_config());
    if json {
        println!("{}", render::health_value(&h));
    } else {
        print!("{}", render::health_text(&h));
    }
    Ok(if h.bells.is_empty() { 0 } else { 1 })
}

pub fn cmd_log(ws: &Workspace, type_prefix: Option<String>, limit: usize, json: bool) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    let filtered: Vec<&Envelope> = events
        .iter()
        .filter(|e| {
            type_prefix
                .as_deref()
                .is_none_or(|p| e.event_type.starts_with(p))
        })
        .collect();
    let tail = &filtered[filtered.len().saturating_sub(limit)..];
    if json {
        let arr = tail
            .iter()
            .map(|e| render::env_value(e))
            .collect::<R<Vec<Value>>>()?;
        println!("{}", Value::Array(arr));
    } else if tail.is_empty() {
        println!("イベントはない。");
    } else {
        for e in tail {
            println!("{}", render::log_line(e));
        }
    }
    Ok(0)
}

pub fn cmd_verify(ws: &Workspace, evidence: bool, deep: bool) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let root = if evidence || deep {
        Some(ws.root.as_path())
    } else {
        None
    };
    let report = ledger
        .verify(root, deep)
        .map_err(|e| format!("検査に失敗: {e}"))?;
    if report.ok() {
        println!("✓ 台帳は健全({} イベント)", report.events);
        Ok(0)
    } else {
        println!(
            "✗ 問題 {} 件({} イベント):",
            report.issues.len(),
            report.events
        );
        for i in &report.issues {
            println!("  行 {} [{}] {}", i.line, i.class, i.message);
        }
        Ok(1)
    }
}

// ---- ゲート(claim ≠ verified) ----

fn find_claim(events: &[Envelope], claim_id: &str) -> R<Envelope> {
    let claim = events
        .iter()
        .find(|e| e.id == claim_id)
        .ok_or_else(|| format!("イベントが見つからない: {claim_id}"))?;
    if claim.event_type != "claim.declare" {
        return Err(format!(
            "{claim_id} は claim.declare ではない(type={})",
            claim.event_type
        ));
    }
    Ok(claim.clone())
}

pub fn cmd_gate_declare(
    ws: &Workspace,
    title: String,
    loop_slug: Option<String>,
    confidence: Option<f64>,
    files: &[PathBuf],
    store: bool,
) -> R<i32> {
    if files.is_empty() {
        return Err(
            "claim.declare には証拠(--evidence FILE)が最低1件必要(契約 不変量2)。\
             要約・伝聞・自己評価は証拠にならない"
                .into(),
        );
    }
    if let Some(c) = confidence {
        if !c.is_finite() {
            return Err("--confidence は有限の数でなければならない".into());
        }
    }
    let evidence = collect_evidence(ws, files, store)?;
    let body = Body::new()
        .set("title", title)
        .set_opt("loop", loop_slug)
        .set_opt("confidence", confidence)
        .into_value();
    let ledger = open_ledger(ws)?;
    append(ws, &ledger, "claim.declare", body, evidence, None)?;
    if confidence.is_some() {
        println!("注: confidence は記録されるが、判定には使われない(契約 G-2)");
    }
    Ok(0)
}

pub fn cmd_gate_check(ws: &Workspace, claim_id: &str) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    let claim = find_claim(&events, claim_id)?;
    let title = claim
        .body
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(無題)");
    println!("主張: {title}");
    println!(
        "宣言: actor={} ts={} id={}",
        claim.actor, claim.ts, claim.id
    );
    println!("証拠 {} 件:", claim.evidence.len());
    let mut unresolved = 0usize;
    if claim.evidence.is_empty() {
        unresolved += 1;
        println!("  ✗ 証拠が 0 件(契約 不変量2: 主張は一次証拠へ結ぶ)");
    }
    for ev in &claim.evidence {
        match &ev.uri {
            None => {
                unresolved += 1;
                println!("  ✗ (uri なし)sha256={}", ev.sha256);
            }
            Some(uri) => {
                let path = ws.root.join(uri);
                if !path.is_file() {
                    unresolved += 1;
                    println!("  ✗ {uri}: ファイルが見つからない");
                } else {
                    match banto_kernel::hash_file(&path) {
                        Ok((h, _)) if h == ev.sha256 => println!("  ✓ {uri}"),
                        Ok(_) => {
                            unresolved += 1;
                            println!("  ✗ {uri}: ハッシュ不一致");
                        }
                        Err(e) => {
                            unresolved += 1;
                            println!("  ✗ {uri}: 読めない({e})");
                        }
                    }
                }
            }
        }
    }
    // 契約 不変量3: 成立を与えられるのは独立した actor の claim.verify だけ。
    // 汎用 append で書かれた同一 actor の claim.verify は判定から除外する
    // (reopen は成立を取り消す向きなので actor を問わない)。
    let claim_actor = claim.actor.clone();
    let mut self_verifies = 0usize;
    let judgments: Vec<&Envelope> = events
        .iter()
        .filter(|e| {
            if e.body.get("claim").and_then(|v| v.as_str()) != Some(claim_id) {
                return false;
            }
            match e.event_type.as_str() {
                "claim.reopen" => true,
                "claim.verify" if e.actor == claim_actor => {
                    self_verifies += 1;
                    false
                }
                "claim.verify" => true,
                _ => false,
            }
        })
        .collect();
    if self_verifies > 0 {
        println!(
            "⚠ 宣言と同じ actor による claim.verify が {self_verifies} 件 — 契約 不変量3 により判定から除外"
        );
    }
    if judgments.is_empty() {
        println!("判定: 未検証(宣言のみ。独立した actor の `banto gate verify` を待つ)");
    } else {
        for j in &judgments {
            let detail = match j.event_type.as_str() {
                "claim.verify" => format!(
                    "verdict={}",
                    j.body
                        .get("verdict")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                ),
                _ => format!(
                    "reason={}",
                    j.body.get("reason").and_then(|v| v.as_str()).unwrap_or("?")
                ),
            };
            println!("  {} {} by {}({})", j.event_type, detail, j.actor, j.ts);
        }
        let last = judgments.last().expect("judgments is non-empty");
        let latest = match (
            last.event_type.as_str(),
            last.body.get("verdict").and_then(|v| v.as_str()),
        ) {
            ("claim.verify", Some("pass")) => "成立(pass)",
            ("claim.verify", _) => "不成立(fail)",
            _ => "開き直し(reopen)",
        };
        println!("最新判定: {latest}");
    }
    if unresolved > 0 {
        println!("✗ 解決しない証拠が {unresolved} 件");
        Ok(1)
    } else {
        println!("✓ 証拠はすべて解決");
        Ok(0)
    }
}

pub fn cmd_gate_verify(
    ws: &Workspace,
    claim_id: &str,
    verdict: String,
    reason: Option<String>,
    files: &[PathBuf],
) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    let claim = find_claim(&events, claim_id)?;
    if claim.actor == ws.actor {
        return Err(format!(
            "宣言と同じ actor '{}' は検証できない(契約 不変量3)。--actor で独立した書き手を指定する",
            ws.actor
        ));
    }
    let evidence = collect_evidence(ws, files, false)?;
    let body = Body::new()
        .set("claim", claim_id)
        .set("verdict", verdict)
        .set_opt("reason", reason)
        .into_value();
    append(ws, &ledger, "claim.verify", body, evidence, None)?;
    Ok(0)
}

pub fn cmd_gate_reopen(ws: &Workspace, claim_id: &str, reason: String) -> R<i32> {
    let ledger = open_ledger(ws)?;
    let events = read_events(&ledger)?;
    find_claim(&events, claim_id)?;
    let body = Body::new()
        .set("claim", claim_id)
        .set("reason", reason)
        .into_value();
    append(ws, &ledger, "claim.reopen", body, vec![], None)?;
    Ok(0)
}
