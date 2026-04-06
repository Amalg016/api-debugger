//! Recursive JSON diff engine.
//!
//! Compares two `serde_json::Value` trees and produces a structured
//! `DiffResult` listing every added, removed, and changed field with
//! its dot-separated path (e.g. `"user.address.city"`).

use serde::Serialize;
use serde_json::Value;

// ─────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────

/// The complete result of comparing two JSON values.
#[derive(Debug, Serialize)]
pub struct DiffResult {
    /// `true` when old == new (no changes at all).
    pub identical: bool,
    /// Fields present in `new` but absent in `old`.
    pub added: Vec<DiffEntry>,
    /// Fields present in `old` but absent in `new`.
    pub removed: Vec<DiffEntry>,
    /// Fields present in both but with different values.
    pub changed: Vec<ChangedEntry>,
}

/// A single added or removed field.
#[derive(Debug, Serialize)]
pub struct DiffEntry {
    /// Dot-separated path, e.g. `"data.users[0].name"`.
    pub path: String,
    /// The value at that path.
    pub value: Value,
}

/// A field whose value differs between old and new.
#[derive(Debug, Serialize)]
pub struct ChangedEntry {
    pub path: String,
    pub old_value: Value,
    pub new_value: Value,
}

// ─────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────

/// Compare two JSON values and return a structured diff.
///
/// Works recursively on objects and arrays. Scalars and type mismatches
/// at the same path are reported as "changed".
pub fn diff_json(old: &Value, new: &Value) -> DiffResult {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    diff_recursive(old, new, String::new(), &mut added, &mut removed, &mut changed);

    DiffResult {
        identical: added.is_empty() && removed.is_empty() && changed.is_empty(),
        added,
        removed,
        changed,
    }
}

// ─────────────────────────────────────────────────────────────────────
// Recursive walker
// ─────────────────────────────────────────────────────────────────────

fn diff_recursive(
    old: &Value,
    new: &Value,
    path: String,
    added: &mut Vec<DiffEntry>,
    removed: &mut Vec<DiffEntry>,
    changed: &mut Vec<ChangedEntry>,
) {
    match (old, new) {
        // ── Both are objects → compare keys ──────────────────────────
        (Value::Object(old_map), Value::Object(new_map)) => {
            // Keys in new but not old → added
            for (key, new_val) in new_map {
                let child_path = join_path(&path, key);
                match old_map.get(key) {
                    Some(old_val) => {
                        diff_recursive(old_val, new_val, child_path, added, removed, changed);
                    }
                    None => {
                        added.push(DiffEntry {
                            path: child_path,
                            value: new_val.clone(),
                        });
                    }
                }
            }
            // Keys in old but not new → removed
            for (key, old_val) in old_map {
                if !new_map.contains_key(key) {
                    removed.push(DiffEntry {
                        path: join_path(&path, key),
                        value: old_val.clone(),
                    });
                }
            }
        }

        // ── Both are arrays → compare element-by-element ────────────
        (Value::Array(old_arr), Value::Array(new_arr)) => {
            let max_len = old_arr.len().max(new_arr.len());
            for i in 0..max_len {
                let child_path = format!("{path}[{i}]");
                match (old_arr.get(i), new_arr.get(i)) {
                    (Some(o), Some(n)) => {
                        diff_recursive(o, n, child_path, added, removed, changed);
                    }
                    (None, Some(n)) => {
                        added.push(DiffEntry {
                            path: child_path,
                            value: n.clone(),
                        });
                    }
                    (Some(o), None) => {
                        removed.push(DiffEntry {
                            path: child_path,
                            value: o.clone(),
                        });
                    }
                    (None, None) => unreachable!(),
                }
            }
        }

        // ── Leaf values or type mismatch → compare directly ─────────
        _ => {
            if old != new {
                let p = if path.is_empty() { "(root)".to_string() } else { path };
                changed.push(ChangedEntry {
                    path: p,
                    old_value: old.clone(),
                    new_value: new.clone(),
                });
            }
        }
    }
}

/// Build a dot-separated path, e.g. `"user" + "name"` → `"user.name"`.
fn join_path(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identical_values() {
        let v = json!({"a": 1, "b": "hello"});
        let result = diff_json(&v, &v);
        assert!(result.identical);
    }

    #[test]
    fn detects_added_fields() {
        let old = json!({"a": 1});
        let new = json!({"a": 1, "b": 2});
        let result = diff_json(&old, &new);
        assert!(!result.identical);
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.added[0].path, "b");
    }

    #[test]
    fn detects_removed_fields() {
        let old = json!({"a": 1, "b": 2});
        let new = json!({"a": 1});
        let result = diff_json(&old, &new);
        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.removed[0].path, "b");
    }

    #[test]
    fn detects_changed_values() {
        let old = json!({"a": 1, "b": "old"});
        let new = json!({"a": 1, "b": "new"});
        let result = diff_json(&old, &new);
        assert_eq!(result.changed.len(), 1);
        assert_eq!(result.changed[0].path, "b");
        assert_eq!(result.changed[0].old_value, json!("old"));
        assert_eq!(result.changed[0].new_value, json!("new"));
    }

    #[test]
    fn nested_objects() {
        let old = json!({"user": {"name": "Alice", "age": 30}});
        let new = json!({"user": {"name": "Bob",   "age": 30, "email": "bob@x.com"}});
        let result = diff_json(&old, &new);
        assert_eq!(result.changed.len(), 1);
        assert_eq!(result.changed[0].path, "user.name");
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.added[0].path, "user.email");
    }

    #[test]
    fn array_diffs() {
        let old = json!([1, 2, 3]);
        let new = json!([1, 99, 3, 4]);
        let result = diff_json(&old, &new);
        assert_eq!(result.changed.len(), 1); // [1] changed
        assert_eq!(result.added.len(), 1);   // [3] added
    }
}
