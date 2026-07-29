//! 正準形 JSON。契約 v1: UTF-8、キーはバイト順ソート、空白なし、非有限数禁止。
//!
//! 文字列エスケープは serde_json の既定に従い、テストベクタで固定する。
//! 台帳の各行は「その行をパースした値の正準形」とバイト単位で一致しなければならない
//! (重複キー・空白・キー順の細工はこの検査で落ちる)。

use crate::{KernelError, Result};
use serde_json::Value;

pub fn canonical_string(value: &Value) -> Result<String> {
    let mut out = String::new();
    write_value(value, &mut out)?;
    Ok(out)
}

fn write_value(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if !f.is_finite() {
                    return Err(KernelError::Canonical("non-finite number".into()));
                }
            }
            out.push_str(&n.to_string());
        }
        Value::String(s) => write_json_string(s, out)?,
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(key, out)?;
                out.push(':');
                write_value(&map[key.as_str()], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn write_json_string(s: &str, out: &mut String) -> Result<()> {
    let encoded = serde_json::to_string(s)
        .map_err(|e| KernelError::Canonical(format!("string encode: {e}")))?;
    out.push_str(&encoded);
    Ok(())
}
