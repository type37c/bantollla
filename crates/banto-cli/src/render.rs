//! 導出ビューの整形。テキストは人間(日本語)向け、JSON は AI 執務層向け。
//!
//! JSON のキーは安定契約として扱う: 変更は AI 執務層との破壊的変更になる。

use banto_kernel::fold::Brief;
use banto_kernel::{BriefConfig, Envelope, Health, LoopState};
use serde_json::{json, Value};

pub fn loop_value(l: &LoopState) -> Value {
    json!({
        "slug": l.slug,
        "title": l.title,
        "kind": l.kind,
        "status": l.status.label(),
        "next": l.next_action,
        "opened_ts": l.opened_ts,
        "last_activity_ts": l.last_activity_ts,
        "last_note": l.last_note,
        "touch_count": l.touch_count,
    })
}

pub fn loop_line(l: &LoopState) -> String {
    format!(
        "[{}] {} — {}({})次の一手: {}",
        l.status.label(),
        l.slug,
        l.title,
        l.kind,
        l.next_action.as_deref().unwrap_or("(未設定)")
    )
}

pub fn brief_value(b: &Brief, cfg: &BriefConfig) -> Value {
    let active: Vec<Value> = b
        .active
        .iter()
        .map(|lb| {
            let mut v = loop_value(&lb.state);
            v["days_idle"] = json!(lb.days_idle);
            v["stalled"] = json!(lb.stalled);
            v
        })
        .collect();
    json!({
        "now": cfg.now.to_rfc3339(),
        "wip_limit": cfg.wip_limit,
        "wip_exceeded": b.wip_exceeded,
        "active": active,
        "parked": b.parked.iter().map(loop_value).collect::<Vec<Value>>(),
    })
}

pub fn brief_text(b: &Brief, cfg: &BriefConfig) -> String {
    let mut out = String::new();
    if b.active.is_empty() {
        out.push_str("開いたループはない。\n");
    } else {
        out.push_str(&format!("稼働中 {} 本(放置が長い順):\n", b.active.len()));
        for lb in &b.active {
            let marker = if lb.stalled { "⚠" } else { "・" };
            out.push_str(&format!(
                "  {} {} — {}({}、放置 {} 日)\n",
                marker, lb.state.slug, lb.state.title, lb.state.kind, lb.days_idle
            ));
            out.push_str(&format!(
                "      次の一手: {}\n",
                lb.state.next_action.as_deref().unwrap_or("(未設定)")
            ));
        }
    }
    out.push_str(&format!("保留場: {} 件\n", b.parked.len()));
    if b.wip_exceeded {
        out.push_str(&format!(
            "⚠ WIP 超過: 稼働中プロジェクトが上限 {} 本を超えている。どれかを保留か完了へ。\n",
            cfg.wip_limit
        ));
    }
    out
}

pub fn health_value(h: &Health) -> Value {
    json!({
        "total_events": h.total_events,
        "loops": {
            "active": h.active,
            "parked": h.parked,
            "closed": h.closed,
            "killed": h.killed,
        },
        "days_since_journal": h.days_since_journal,
        "events_since_last_close": h.events_since_last_close,
        "bells": h.bells,
    })
}

pub fn health_text(h: &Health) -> String {
    let mut out = String::new();
    out.push_str(&format!("イベント総数: {}\n", h.total_events));
    out.push_str(&format!(
        "ループ: 稼働中 {} / 保留 {} / 完了 {} / 殺 {}\n",
        h.active, h.parked, h.closed, h.killed
    ));
    match h.days_since_journal {
        None => out.push_str("朝の統括: 記録なし\n"),
        Some(d) => out.push_str(&format!("朝の統括: 最終から {d} 日\n")),
    }
    out.push_str(&format!(
        "最後の close からのイベント数: {}\n",
        h.events_since_last_close
    ));
    if h.bells.is_empty() {
        out.push_str("鐘: 鳴っていない\n");
    } else {
        out.push_str("鐘:\n");
        for b in &h.bells {
            out.push_str(&format!("  ⚠ {b}\n"));
        }
    }
    out
}

/// 台帳の1行と同じ正準形を JSON 値として返す(log --json 用)。
pub fn env_value(e: &Envelope) -> Result<Value, String> {
    let line = e
        .to_line()
        .map_err(|err| format!("イベントの整形に失敗: {err}"))?;
    serde_json::from_str(&line).map_err(|err| format!("イベントの整形に失敗: {err}"))
}

pub fn log_line(e: &Envelope) -> String {
    let body = serde_json::to_string(&e.body).unwrap_or_else(|_| "{}".into());
    let mut compact: String = body.chars().take(80).collect();
    if body.chars().count() > 80 {
        compact.push('…');
    }
    format!(
        "[{}] {} {} {} {}",
        e.seq, e.ts, e.actor, e.event_type, compact
    )
}
