//! User settings: one JSON object, stored in the database and changed by JSON
//! merge patches (`PROTO-CORE-API`, Settings).
//!
//! Settings Core reads itself have an accessor here that applies the default,
//! so the default lives in one place. `settings.get` returns only what the
//! user has set; defaults are not written into the object.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Applies a JSON merge patch (RFC 7386) to `target`.
///
/// For each key of an object patch: `null` removes the key, an object merges
/// recursively, and anything else replaces the value. A patch that is not an
/// object replaces `target` entirely.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let Value::Object(target) = target else {
        unreachable!("target was just made an object")
    };
    for (key, value) in patch {
        if value.is_null() {
            target.remove(key);
        } else {
            merge_patch(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

/// Checks the settings Core understands have the right types, so a bad patch
/// is refused before it is stored rather than failing later.
pub fn validate(settings: &Map<String, Value>) -> Result<(), String> {
    if let Some(workspaces) = settings.get("workspaces") {
        let Some(workspaces) = workspaces.as_object() else {
            return Err("workspaces must be an object".into());
        };
        if let Some(root) = workspaces.get("root") {
            match root.as_str() {
                Some(root) if Path::new(root).is_absolute() => {}
                _ => return Err("workspaces.root must be an absolute path".into()),
            }
        }
    }
    Ok(())
}

/// `workspaces.root`: the directory new worktrees go under. Defaults to
/// `worktrees` in Core's data directory, which keeps them outside every
/// project tree.
pub fn workspaces_root(settings: &Map<String, Value>, data_dir: &Path) -> PathBuf {
    settings
        .get("workspaces")
        .and_then(|w| w.get("root"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("worktrees"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The examples from RFC 7386, appendix A.
    #[test]
    fn rfc_7386_examples() {
        let cases = [
            (json!({"a":"b"}), json!({"a":"c"}), json!({"a":"c"})),
            (json!({"a":"b"}), json!({"b":"c"}), json!({"a":"b","b":"c"})),
            (json!({"a":"b"}), json!({"a":null}), json!({})),
            (
                json!({"a":"b","b":"c"}),
                json!({"a":null}),
                json!({"b":"c"}),
            ),
            (json!({"a":["b"]}), json!({"a":"c"}), json!({"a":"c"})),
            (json!({"a":"c"}), json!({"a":["b"]}), json!({"a":["b"]})),
            (
                json!({"a":{"b":"c"}}),
                json!({"a":{"b":"d","c":null}}),
                json!({"a":{"b":"d"}}),
            ),
            (json!({"a":[{"b":"c"}]}), json!({"a":[1]}), json!({"a":[1]})),
            (json!(["a", "b"]), json!(["c", "d"]), json!(["c", "d"])),
            (json!({"a":"b"}), json!(["c"]), json!(["c"])),
            (json!({"a":"foo"}), json!(null), json!(null)),
            (json!({"a":"foo"}), json!("bar"), json!("bar")),
            (json!({"e":null}), json!({"a":1}), json!({"e":null,"a":1})),
            (json!([1, 2]), json!({"a":"b","c":null}), json!({"a":"b"})),
            (
                json!({}),
                json!({"a":{"bb":{"ccc":null}}}),
                json!({"a":{"bb":{}}}),
            ),
        ];
        for (mut target, patch, expected) in cases {
            merge_patch(&mut target, &patch);
            assert_eq!(target, expected, "patch {patch}");
        }
    }

    #[test]
    fn workspaces_root_defaults_under_the_data_dir() {
        let data = Path::new("/data");
        assert_eq!(workspaces_root(&Map::new(), data), data.join("worktrees"));
        let set = json!({ "workspaces": { "root": "/elsewhere" } });
        assert_eq!(
            workspaces_root(set.as_object().unwrap(), data),
            PathBuf::from("/elsewhere")
        );
    }

    #[test]
    fn a_relative_root_is_invalid() {
        let bad = json!({ "workspaces": { "root": "relative/dir" } });
        assert!(validate(bad.as_object().unwrap()).is_err());
        let bad = json!({ "workspaces": 3 });
        assert!(validate(bad.as_object().unwrap()).is_err());
    }
}
