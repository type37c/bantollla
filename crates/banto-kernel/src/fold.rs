//! 導出層: 台帳イベントの畳み込み。
//!
//! ここにあるものはすべて使い捨てのビューであり、権威は台帳にしかない。
//! 番頭の意味論(ループ、朝の brief、健全性の鐘)はこのモジュールに住む。

use crate::event::Envelope;
use chrono::{DateTime, FixedOffset};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStatus {
    Active,
    Parked,
    Closed,
    Killed,
}

impl LoopStatus {
    pub fn label(&self) -> &'static str {
        match self {
            LoopStatus::Active => "active",
            LoopStatus::Parked => "parked",
            LoopStatus::Closed => "closed",
            LoopStatus::Killed => "killed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoopState {
    pub slug: String,
    pub title: String,
    pub kind: String,
    pub status: LoopStatus,
    pub next_action: Option<String>,
    pub opened_ts: String,
    pub last_activity_ts: String,
    pub last_note: Option<String>,
    pub touch_count: u64,
}

fn body_str(env: &Envelope, key: &str) -> Option<String> {
    env.body
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// ループ状態への畳み込み。不正な形のイベントは黙って読み飛ばす
/// (構造の検査は verify の仕事。導出は全域関数であるべき)。
pub fn fold_loops(events: &[Envelope]) -> BTreeMap<String, LoopState> {
    let mut loops: BTreeMap<String, LoopState> = BTreeMap::new();
    for env in events {
        let explicit = env.event_type.strip_prefix("loop.");
        let slug = match explicit {
            Some(_) => body_str(env, "slug"),
            None => body_str(env, "loop"),
        };
        let Some(slug) = slug else { continue };
        match explicit {
            Some("open") => {
                let entry = loops.entry(slug.clone()).or_insert_with(|| LoopState {
                    slug: slug.clone(),
                    title: String::new(),
                    kind: "project".into(),
                    status: LoopStatus::Active,
                    next_action: None,
                    opened_ts: env.ts.clone(),
                    last_activity_ts: env.ts.clone(),
                    last_note: None,
                    touch_count: 0,
                });
                if let Some(title) = body_str(env, "title") {
                    entry.title = title;
                }
                if let Some(kind) = body_str(env, "kind") {
                    entry.kind = kind;
                }
                if let Some(next) = body_str(env, "next") {
                    entry.next_action = Some(next);
                }
                entry.status = LoopStatus::Active;
                entry.last_activity_ts = env.ts.clone();
            }
            Some(verb) => {
                let Some(entry) = loops.get_mut(&slug) else {
                    continue;
                };
                entry.last_activity_ts = env.ts.clone();
                match verb {
                    "next" => {
                        if let Some(action) = body_str(env, "action") {
                            entry.next_action = Some(action);
                        }
                    }
                    "touch" => {
                        entry.touch_count += 1;
                        if let Some(note) = body_str(env, "note") {
                            entry.last_note = Some(note);
                        }
                    }
                    "close" => entry.status = LoopStatus::Closed,
                    "park" => entry.status = LoopStatus::Parked,
                    "kill" => entry.status = LoopStatus::Killed,
                    _ => {}
                }
            }
            None => {
                // body.loop で任意のイベントをループへの活動として数える(契約の慣例)。
                if let Some(entry) = loops.get_mut(&slug) {
                    entry.last_activity_ts = env.ts.clone();
                    entry.touch_count += 1;
                }
            }
        }
    }
    loops
}

#[derive(Debug, Clone)]
pub struct BriefConfig {
    pub now: DateTime<FixedOffset>,
    pub stall_days: i64,
    pub habit_stall_days: i64,
    pub wip_limit: usize,
}

#[derive(Debug, Clone)]
pub struct LoopBrief {
    pub state: LoopState,
    pub days_idle: i64,
    pub stalled: bool,
}

#[derive(Debug)]
pub struct Brief {
    pub active: Vec<LoopBrief>,
    pub parked: Vec<LoopState>,
    pub wip_exceeded: bool,
}

fn parse_ts(ts: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(ts).ok()
}

/// 朝の brief。停滞したものから順に並ぶ(番頭は一番放置されたものから差し出す)。
pub fn brief(events: &[Envelope], config: &BriefConfig) -> Brief {
    let loops = fold_loops(events);
    let mut active = Vec::new();
    let mut parked = Vec::new();
    for state in loops.into_values() {
        match state.status {
            LoopStatus::Active => {
                let days_idle = parse_ts(&state.last_activity_ts)
                    .map(|t| (config.now - t).num_days())
                    .unwrap_or(0);
                let threshold = if state.kind == "habit" {
                    config.habit_stall_days
                } else {
                    config.stall_days
                };
                active.push(LoopBrief {
                    stalled: days_idle >= threshold,
                    days_idle,
                    state,
                });
            }
            LoopStatus::Parked => parked.push(state),
            _ => {}
        }
    }
    active.sort_by_key(|l| std::cmp::Reverse(l.days_idle));
    let wip_exceeded = active.iter().filter(|l| l.state.kind != "habit").count() > config.wip_limit;
    Brief {
        active,
        parked,
        wip_exceeded,
    }
}

#[derive(Debug)]
pub struct Health {
    pub total_events: usize,
    pub active: usize,
    pub parked: usize,
    pub closed: usize,
    pub killed: usize,
    pub days_since_journal: Option<i64>,
    pub events_since_last_close: usize,
    pub bells: Vec<String>,
}

/// 健全性の鐘。bgit の死因(儀式肥大・黙った停止)を監視対象の不変量にする。
pub fn health(events: &[Envelope], config: &BriefConfig) -> Health {
    let loops = fold_loops(events);
    let count = |s: LoopStatus| loops.values().filter(|l| l.status == s).count();
    let last_journal = events
        .iter()
        .rev()
        .find(|e| matches!(e.event_type.as_str(), "journal.entry" | "video.log"))
        .and_then(|e| parse_ts(&e.ts));
    let days_since_journal = last_journal.map(|t| (config.now - t).num_days());
    let events_since_last_close = events
        .iter()
        .rev()
        .take_while(|e| e.event_type != "loop.close")
        .count();

    let mut bells = Vec::new();
    match days_since_journal {
        None => bells.push("朝の統括がまだ一度も台帳に入っていない".into()),
        Some(d) if d >= 2 => bells.push(format!("朝の統括が {d} 日途絶えている")),
        _ => {}
    }
    let active = count(LoopStatus::Active);
    let active_projects = loops
        .values()
        .filter(|l| l.status == LoopStatus::Active && l.kind != "habit")
        .count();
    if active_projects > config.wip_limit {
        bells.push(format!(
            "稼働中プロジェクトが {active_projects} 本ある(上限 {})。どれかを保留場へ",
            config.wip_limit
        ));
    }
    if !events.is_empty() && events_since_last_close > 200 {
        bells.push(format!(
            "最後に何かを閉じてから {events_since_last_close} イベント経過。台帳が回っているのに何も閉じていない"
        ));
    }
    Health {
        total_events: events.len(),
        active,
        parked: count(LoopStatus::Parked),
        closed: count(LoopStatus::Closed),
        killed: count(LoopStatus::Killed),
        days_since_journal,
        events_since_last_close,
        bells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SCHEMA;
    use serde_json::json;

    fn ev(seq: u64, ts: &str, event_type: &str, body: serde_json::Value) -> Envelope {
        let mut env = Envelope {
            schema: SCHEMA.into(),
            seq,
            prev: if seq == 0 { None } else { Some("0".repeat(64)) },
            ts: ts.into(),
            actor: "test".into(),
            event_type: event_type.into(),
            body,
            evidence: vec![],
            op: None,
            id: String::new(),
        };
        env.id = env.compute_id().unwrap();
        env
    }

    fn now(ts: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(ts).unwrap()
    }

    fn config(now_ts: &str) -> BriefConfig {
        BriefConfig {
            now: now(now_ts),
            stall_days: 4,
            habit_stall_days: 2,
            wip_limit: 2,
        }
    }

    #[test]
    fn loop_lifecycle_folds_correctly() {
        let events = vec![
            ev(
                0,
                "2026-07-01T09:00:00+09:00",
                "loop.open",
                json!({"slug": "paper", "title": "論文投稿", "next": "著者名を入れる"}),
            ),
            ev(
                1,
                "2026-07-02T09:00:00+09:00",
                "loop.touch",
                json!({"slug": "paper", "note": "少し進めた"}),
            ),
            ev(
                2,
                "2026-07-03T09:00:00+09:00",
                "video.log",
                json!({"filename": "a.mp4", "loop": "paper"}),
            ),
            ev(
                3,
                "2026-07-04T09:00:00+09:00",
                "loop.close",
                json!({"slug": "paper", "outcome": "提出した"}),
            ),
        ];
        let loops = fold_loops(&events);
        let paper = &loops["paper"];
        assert_eq!(paper.status, LoopStatus::Closed);
        assert_eq!(paper.touch_count, 2);
        assert_eq!(paper.last_activity_ts, "2026-07-04T09:00:00+09:00");
        assert_eq!(paper.next_action.as_deref(), Some("著者名を入れる"));
    }

    #[test]
    fn brief_flags_stalls_and_sorts_most_idle_first() {
        let events = vec![
            ev(
                0,
                "2026-07-01T09:00:00+09:00",
                "loop.open",
                json!({"slug": "old", "title": "古い"}),
            ),
            ev(
                1,
                "2026-07-16T09:00:00+09:00",
                "loop.open",
                json!({"slug": "fresh", "title": "新しい"}),
            ),
            ev(
                2,
                "2026-07-15T09:00:00+09:00",
                "loop.open",
                json!({"slug": "video", "title": "ビデオ", "kind": "habit"}),
            ),
        ];
        let b = brief(&events, &config("2026-07-17T09:00:00+09:00"));
        assert_eq!(b.active[0].state.slug, "old");
        assert!(b.active[0].stalled); // 16日放置 >= 4
        assert!(
            !b.active
                .iter()
                .find(|l| l.state.slug == "fresh")
                .unwrap()
                .stalled
        );
        assert!(
            b.active
                .iter()
                .find(|l| l.state.slug == "video")
                .unwrap()
                .stalled
        ); // habit は2日で停滞
        assert!(!b.wip_exceeded); // habit は WIP に数えない: プロジェクト2本は上限内
    }

    #[test]
    fn health_rings_bells() {
        let events = vec![
            ev(
                0,
                "2026-07-01T09:00:00+09:00",
                "loop.open",
                json!({"slug": "a", "title": "A"}),
            ),
            ev(
                1,
                "2026-07-01T09:10:00+09:00",
                "loop.open",
                json!({"slug": "b", "title": "B"}),
            ),
            ev(
                2,
                "2026-07-01T09:20:00+09:00",
                "loop.open",
                json!({"slug": "c", "title": "C"}),
            ),
            ev(
                3,
                "2026-07-10T09:00:00+09:00",
                "video.log",
                json!({"filename": "v.mp4"}),
            ),
        ];
        let h = health(&events, &config("2026-07-17T09:00:00+09:00"));
        assert_eq!(h.active, 3);
        assert!(h.bells.iter().any(|b| b.contains("朝の統括が 7 日")));
        assert!(h
            .bells
            .iter()
            .any(|b| b.contains("稼働中プロジェクトが 3 本")));
    }
}
