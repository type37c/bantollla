//! 封筒(envelope)。契約 v1 の実装。
//!
//! フィールド規則は docs/contract_v1.md を参照。`op` と evidence の
//! `uri`/`media_type`/`bytes` は未設定時に省略、他は常に存在する。

use crate::canonical::canonical_string;
use crate::{hash_bytes, KernelError, Result};
use serde_json::{Map, Value};

pub const SCHEMA: &str = "banto/1";

const MAX_ACTOR_LEN: usize = 200;
const MAX_TYPE_LEN: usize = 100;
const MAX_OP_LEN: usize = 200;
const MAX_URI_LEN: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub sha256: String,
    pub uri: Option<String>,
    pub media_type: Option<String>,
    pub bytes: Option<u64>,
}

impl Evidence {
    fn to_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("sha256".into(), Value::String(self.sha256.clone()));
        if let Some(uri) = &self.uri {
            map.insert("uri".into(), Value::String(uri.clone()));
        }
        if let Some(mt) = &self.media_type {
            map.insert("media_type".into(), Value::String(mt.clone()));
        }
        if let Some(bytes) = self.bytes {
            map.insert("bytes".into(), Value::Number(bytes.into()));
        }
        Value::Object(map)
    }

    fn from_value(value: &Value) -> Result<Self> {
        let map = value
            .as_object()
            .ok_or_else(|| KernelError::Validation("evidence entry must be an object".into()))?;
        for key in map.keys() {
            if !matches!(key.as_str(), "sha256" | "uri" | "media_type" | "bytes") {
                return Err(KernelError::Validation(format!(
                    "unknown evidence field: {key}"
                )));
            }
        }
        let sha256 = req_str(map, "sha256", "evidence.sha256")?;
        let uri = opt_str(map, "uri", "evidence.uri")?;
        let media_type = opt_str(map, "media_type", "evidence.media_type")?;
        let bytes = match map.get("bytes") {
            None => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| {
                KernelError::Validation("evidence.bytes must be a non-negative integer".into())
            })?),
        };
        Ok(Evidence {
            sha256,
            uri,
            media_type,
            bytes,
        })
    }

    fn validate(&self) -> Result<()> {
        if !is_hex64(&self.sha256) {
            return Err(KernelError::Validation(format!(
                "evidence.sha256 must be 64 lowercase hex chars, got: {}",
                self.sha256
            )));
        }
        if let Some(uri) = &self.uri {
            if uri.is_empty() || uri.len() > MAX_URI_LEN || uri.chars().any(|c| c.is_control()) {
                return Err(KernelError::Validation("invalid evidence.uri".into()));
            }
        }
        if let Some(mt) = &self.media_type {
            if mt.is_empty() || mt.len() > 200 || mt.chars().any(|c| c.is_control()) {
                return Err(KernelError::Validation(
                    "invalid evidence.media_type".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub schema: String,
    pub seq: u64,
    pub prev: Option<String>,
    pub ts: String,
    pub actor: String,
    pub event_type: String,
    pub body: Value,
    pub evidence: Vec<Evidence>,
    pub op: Option<String>,
    pub id: String,
}

impl Envelope {
    fn to_value_without_id(&self) -> Value {
        let mut map = Map::new();
        map.insert("schema".into(), Value::String(self.schema.clone()));
        map.insert("seq".into(), Value::Number(self.seq.into()));
        map.insert(
            "prev".into(),
            match &self.prev {
                Some(p) => Value::String(p.clone()),
                None => Value::Null,
            },
        );
        map.insert("ts".into(), Value::String(self.ts.clone()));
        map.insert("actor".into(), Value::String(self.actor.clone()));
        map.insert("type".into(), Value::String(self.event_type.clone()));
        map.insert("body".into(), self.body.clone());
        map.insert(
            "evidence".into(),
            Value::Array(self.evidence.iter().map(Evidence::to_value).collect()),
        );
        if let Some(op) = &self.op {
            map.insert("op".into(), Value::String(op.clone()));
        }
        Value::Object(map)
    }

    /// `id` を除く封筒の正準形 JSON の SHA-256。
    pub fn compute_id(&self) -> Result<String> {
        let canon = canonical_string(&self.to_value_without_id())?;
        Ok(hash_bytes(canon.as_bytes()))
    }

    /// 台帳に書かれる1行(正準形、改行なし)。
    pub fn to_line(&self) -> Result<String> {
        let mut value = self.to_value_without_id();
        value
            .as_object_mut()
            .expect("envelope value is an object")
            .insert("id".into(), Value::String(self.id.clone()));
        canonical_string(&value)
    }

    pub fn from_line(line: &str, line_no: usize) -> Result<Envelope> {
        let value: Value = serde_json::from_str(line).map_err(|e| KernelError::Parse {
            line: line_no,
            msg: e.to_string(),
        })?;
        let map = value.as_object().ok_or_else(|| KernelError::Parse {
            line: line_no,
            msg: "event must be a JSON object".into(),
        })?;
        for key in map.keys() {
            if !matches!(
                key.as_str(),
                "schema"
                    | "seq"
                    | "prev"
                    | "ts"
                    | "actor"
                    | "type"
                    | "body"
                    | "evidence"
                    | "op"
                    | "id"
            ) {
                return Err(KernelError::Parse {
                    line: line_no,
                    msg: format!("unknown envelope field: {key}"),
                });
            }
        }
        let seq = map
            .get("seq")
            .and_then(Value::as_u64)
            .ok_or_else(|| KernelError::Parse {
                line: line_no,
                msg: "seq must be a non-negative integer".into(),
            })?;
        let prev = match map.get("prev") {
            Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            _ => {
                return Err(KernelError::Parse {
                    line: line_no,
                    msg: "prev must be a string or null".into(),
                })
            }
        };
        let evidence = match map.get("evidence") {
            Some(Value::Array(items)) => items
                .iter()
                .map(Evidence::from_value)
                .collect::<Result<Vec<_>>>()?,
            _ => {
                return Err(KernelError::Parse {
                    line: line_no,
                    msg: "evidence must be an array".into(),
                })
            }
        };
        let body = map.get("body").cloned().ok_or_else(|| KernelError::Parse {
            line: line_no,
            msg: "missing body".into(),
        })?;
        Ok(Envelope {
            schema: req_str(map, "schema", "schema")?,
            seq,
            prev,
            ts: req_str(map, "ts", "ts")?,
            actor: req_str(map, "actor", "actor")?,
            event_type: req_str(map, "type", "type")?,
            body,
            evidence,
            op: opt_str(map, "op", "op")?,
            id: req_str(map, "id", "id")?,
        })
    }

    /// 契約 v1 の構造規則を検査する(連鎖の検査は Ledger::verify が行う)。
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            return Err(KernelError::Validation(format!(
                "unknown schema: {} (expected {SCHEMA})",
                self.schema
            )));
        }
        if let Some(prev) = &self.prev {
            if !is_hex64(prev) {
                return Err(KernelError::Validation("prev must be 64 hex chars".into()));
            }
        }
        if (self.seq == 0) != self.prev.is_none() {
            return Err(KernelError::Validation(
                "prev must be null exactly for seq 0".into(),
            ));
        }
        chrono::DateTime::parse_from_rfc3339(&self.ts)
            .map_err(|e| KernelError::Validation(format!("ts is not RFC 3339: {e}")))?;
        if self.actor.is_empty()
            || self.actor.len() > MAX_ACTOR_LEN
            || self.actor.chars().any(|c| c.is_control())
        {
            return Err(KernelError::Validation("invalid actor".into()));
        }
        if !is_valid_type(&self.event_type) {
            return Err(KernelError::Validation(format!(
                "invalid type: {}",
                self.event_type
            )));
        }
        if !self.body.is_object() {
            return Err(KernelError::Validation("body must be a JSON object".into()));
        }
        for entry in &self.evidence {
            entry.validate()?;
        }
        if let Some(op) = &self.op {
            if op.is_empty() || op.len() > MAX_OP_LEN || op.chars().any(|c| c.is_control()) {
                return Err(KernelError::Validation("invalid op".into()));
            }
        }
        if !is_hex64(&self.id) {
            return Err(KernelError::Validation("id must be 64 hex chars".into()));
        }
        let computed = self.compute_id()?;
        if computed != self.id {
            return Err(KernelError::Validation(format!(
                "id mismatch: recorded {}, computed {computed}",
                self.id
            )));
        }
        Ok(())
    }

    /// 冪等キー照合に使う内容の同一性(seq/prev/ts/id は比較しない)。
    pub fn same_content(&self, other: &Envelope) -> bool {
        self.actor == other.actor
            && self.event_type == other.event_type
            && self.body == other.body
            && self.evidence == other.evidence
    }
}

pub fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// type は小文字ドット区切り: 各セグメントは [a-z0-9_-]+、空セグメント禁止。
pub fn is_valid_type(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_TYPE_LEN {
        return false;
    }
    s.split('.').all(|seg| {
        !seg.is_empty()
            && seg
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    })
}

fn req_str(map: &Map<String, Value>, key: &str, label: &str) -> Result<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| KernelError::Validation(format!("{label} must be a string")))
}

fn opt_str(map: &Map<String, Value>, key: &str, label: &str) -> Result<Option<String>> {
    match map.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(KernelError::Validation(format!("{label} must be a string"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Envelope {
        let mut env = Envelope {
            schema: SCHEMA.into(),
            seq: 0,
            prev: None,
            ts: "2026-07-17T09:00:00+09:00".into(),
            actor: "alice".into(),
            event_type: "note.capture".into(),
            body: json!({"text": "最初のメモ"}),
            evidence: vec![],
            op: None,
            id: String::new(),
        };
        env.id = env.compute_id().unwrap();
        env
    }

    #[test]
    fn id_matches_independent_hash_of_manual_canonical_form() {
        let env = sample();
        // 契約 v1 の正準形を手書きし、カーネルと独立に SHA-256 を取る。
        let manual = concat!(
            "{\"actor\":\"alice\",\"body\":{\"text\":\"最初のメモ\"},",
            "\"evidence\":[],\"prev\":null,\"schema\":\"banto/1\",\"seq\":0,",
            "\"ts\":\"2026-07-17T09:00:00+09:00\",\"type\":\"note.capture\"}"
        );
        assert_eq!(env.id, crate::hash_bytes(manual.as_bytes()));
        env.validate().unwrap();
    }

    #[test]
    fn line_roundtrip_is_canonical_and_stable() {
        let env = sample();
        let line = env.to_line().unwrap();
        let parsed = Envelope::from_line(&line, 1).unwrap();
        assert_eq!(parsed, env);
        assert_eq!(parsed.to_line().unwrap(), line);
    }

    #[test]
    fn unknown_envelope_field_is_rejected() {
        let env = sample();
        let line = env.to_line().unwrap();
        let hacked = line.replacen("{", "{\"confidence_gate\":true,", 1);
        assert!(Envelope::from_line(&hacked, 1).is_err());
    }

    #[test]
    fn type_charset_is_enforced() {
        assert!(is_valid_type("video.log"));
        assert!(is_valid_type("loop.open"));
        assert!(is_valid_type("a.b_c-d.e2"));
        assert!(!is_valid_type("Video.log"));
        assert!(!is_valid_type("video..log"));
        assert!(!is_valid_type(".video"));
        assert!(!is_valid_type(""));
        assert!(!is_valid_type("video log"));
    }

    #[test]
    fn tampered_body_fails_validation() {
        let mut env = sample();
        env.body = json!({"text": "改竄されたメモ"});
        assert!(env.validate().is_err());
    }
}
