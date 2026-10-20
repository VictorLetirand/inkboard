//! Tools of the MCP v2 plan (see `context/PLAN_MCP_V2.md`); the module is registered in `tools.rs`.
//!
//! `batch`: several tool calls in one request, on one board, as one undo step. Operations are run by name through `super::apply`, so any
//! tool of the server can be used in a batch without this module knowing the list (except the few that are forbidden).

use super::{Res, obj, page_prop, path_prop};
use inkboard_core::{Editor, Id};
use serde_json::{Value, json};

/// Tools exposed by this module.
pub(super) const NAMES: &[&str] = &["batch"];
/// Those that do not modify the board (a batch may, so the file is always saved after a successful one).
pub(super) const READ_ONLY: &[&str] = &[];

const MAX_OPERATIONS: usize = 200;
/// Tools with side effects outside the board, or that would make the change tracking inconsistent.
const FORBIDDEN: [&str; 6] = ["batch", "create_board", "save_board", "board_changes", "export", "render"];
/// Keys the confinement to `--root` is applied to, by `call()`, on the first level of the arguments only.
const PATH_KEYS: [&str; 3] = ["path", "output", "source"];
const PATHS_REFUSED: &str = "paths cannot be set inside a batch operation; use the batch-level path";
/// Longest list of partial results quoted in the error of an aborted atomic batch.
const QUOTED_RESULTS_MAX: usize = 8000;

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    vec![json!({"name": "batch",
        "description": "Runs several tool calls in ONE request (1 to 200 operations, in order), on one board, as a single undo step in the application. Use it to build or rework a board quickly instead of one call per object. \
            Each operation is {\"tool\": \"add_shape\", \"args\": {...}} with the arguments the tool normally takes (but never `path`, `output` or `source`: the batch-level `path` applies to all). \
            An operation can reuse what an earlier one returned: a string that is EXACTLY \"$N.key\" (N = index from 0 of an earlier operation, then keys or list indexes separated by dots, e.g. \"$0.id\" or \"$3.ids.1\") is replaced by that value of the result of operation N before the operation runs; \
            text that only contains \"$0.id\" inside a longer string stays literal, and a text that is exactly like \"$3.99\" would be taken for a reference. \
            An operation without `page` acts on the batch-level `page` (default: the first page), whatever the previous operations did. \
            `atomic` (default true): at the first error everything already done is undone, the board is unchanged, and the error gives the operation index, its error and the results obtained before the rollback. \
            With `atomic: false` the batch stops at the first error and keeps what succeeded, answering {applied, failed: {index, error}, results} (not an error). \
            Success answers {results: [one result per operation], applied}. Not allowed inside a batch: batch, create_board, save_board, board_changes, export, render.",
        "inputSchema": obj(json!({
            "path": path_prop(),
            "page": page_prop(),
            "atomic": {"type": "boolean", "description": "All or nothing (default true). false: keep the operations done before the first error."},
            "operations": {"type": "array", "minItems": 1, "maxItems": MAX_OPERATIONS,
                "items": {"type": "object", "properties": {
                    "tool": {"type": "string", "description": "Name of the tool to run."},
                    "args": {"type": "object", "description": "Arguments of the tool. Strings \"$N.key\" are replaced by a value of the result of operation N."}
                }, "required": ["tool"]},
                "description": "Operations to run in order, e.g. [{\"tool\": \"add_shape\", \"args\": {\"geom\": \"rect\", \"text\": \"A\"}}, {\"tool\": \"add_shape\", \"args\": {\"geom\": \"rect\", \"text\": \"B\"}}, {\"tool\": \"add_connector\", \"args\": {\"from\": \"$0.id\", \"to\": \"$1.id\"}}]."}
        }), &["operations"])})]
}

/// Runs the tool when this module owns it, `None` otherwise.
pub(super) fn apply(ed: &mut Editor, name: &str, a: &Value) -> Option<Res<Value>> {
    (name == "batch").then(|| batch(ed, a))
}

fn batch(ed: &mut Editor, a: &Value) -> Res<Value> {
    let ops = parse(a)?;
    let atomic = match a.get("atomic") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or("\"atomic\" must be true or false")?,
    };
    // the batch-level page has been selected by `apply`: it is the page of every operation that gives none
    let base = ed.current.clone();
    let mut results = Vec::new();
    let outcome = if atomic { ed.try_transact(|ed| run(ed, &ops, &base, &mut results)) } else { ed.transact(|ed| run(ed, &ops, &base, &mut results)) };
    match outcome {
        Ok(()) => Ok(json!({"applied": results.len(), "results": results})),
        Err((i, error)) if atomic => {
            let mut done = Value::Array(results).to_string();
            if done.chars().count() > QUOTED_RESULTS_MAX {
                done = done.chars().take(QUOTED_RESULTS_MAX).collect::<String>() + "...";
            }
            Err(format!(
                "batch aborted at operation {i} ({}): {error}. The {i} operation(s) before it were rolled back: the board is unchanged. Results obtained before the rollback: {done}",
                ops[i].0
            ))
        }
        Err((i, error)) => Ok(json!({"applied": i, "failed": {"index": i, "tool": ops[i].0, "error": error}, "results": results})),
    }
}

/// Runs the operations in order, stopping at the first error (index and message). A failing operation leaves nothing behind.
fn run(ed: &mut Editor, ops: &[(String, Value)], base: &Id, results: &mut Vec<Value>) -> Result<(), (usize, String)> {
    for (i, (tool, args)) in ops.iter().enumerate() {
        let args =
            resolve_refs(args, results).and_then(|args| if has_path_key(&args) { Err(PATHS_REFUSED.to_owned()) } else { Ok(args) }).map_err(|e| (i, e))?;
        // an operation that gives no `page` starts from the batch page, not from where a previous operation left the editor
        ed.current = if ed.doc.page(base).is_some() { base.clone() } else { ed.doc.pages[0].id.clone() };
        let value = ed.try_transact(|ed| super::apply(ed, tool, &args)).map_err(|e| (i, e))?;
        results.push(value);
    }
    Ok(())
}

/// Validates the whole request before anything runs: shape, count, forbidden tools, forbidden path keys.
fn parse(a: &Value) -> Res<Vec<(String, Value)>> {
    let ops = a.get("operations").and_then(Value::as_array).ok_or("missing \"operations\" parameter (a list of {\"tool\": ..., \"args\": {...}})")?;
    if ops.is_empty() || ops.len() > MAX_OPERATIONS {
        return Err(format!("\"operations\" must hold 1 to {MAX_OPERATIONS} operations (got {})", ops.len()));
    }
    ops.iter()
        .enumerate()
        .map(|(i, op)| {
            let m = op.as_object().ok_or_else(|| format!("operation {i}: expected an object {{\"tool\": ..., \"args\": {{...}}}}"))?;
            if PATH_KEYS.iter().any(|k| m.contains_key(*k)) {
                return Err(format!("operation {i}: {PATHS_REFUSED}"));
            }
            if let Some(k) = m.keys().find(|k| !["tool", "args"].contains(&k.as_str())) {
                return Err(format!("operation {i}: unknown key \"{k}\" (an operation is {{\"tool\": ..., \"args\": {{...}}}})"));
            }
            let tool = m.get("tool").and_then(Value::as_str).ok_or_else(|| format!("operation {i}: missing \"tool\" (the name of the tool to run)"))?;
            if FORBIDDEN.contains(&tool) {
                return Err(format!("operation {i}: tool {tool} cannot be used inside a batch"));
            }
            let args = match m.get("args") {
                None | Some(Value::Null) => json!({}),
                Some(v @ Value::Object(_)) => v.clone(),
                Some(_) => return Err(format!("operation {i}: \"args\" must be an object")),
            };
            if has_path_key(&args) {
                return Err(format!("operation {i}: {PATHS_REFUSED}"));
            }
            Ok((tool.to_owned(), args))
        })
        .collect()
}

/// Whether a key named `path`, `output` or `source` appears anywhere in the value (the tools only read the first level, but a nested one is refused too).
fn has_path_key(v: &Value) -> bool {
    match v {
        Value::Object(m) => m.iter().any(|(k, x)| PATH_KEYS.contains(&k.as_str()) || has_path_key(x)),
        Value::Array(l) => l.iter().any(has_path_key),
        _ => false,
    }
}

/// `"$N.path"` as a whole string: the operation number and the path segments. Anything else (including free text) is not a reference.
fn parse_ref(s: &str) -> Option<(usize, Vec<&str>)> {
    let (n, path) = s.strip_prefix('$')?.split_once('.')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let segments: Vec<&str> = path.split('.').collect();
    let valid = |seg: &&str| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    (segments.iter().all(valid)).then(|| n.parse().ok().map(|n| (n, segments)))?
}

/// Replaces every reference string of the value by the value it designates in the earlier results.
fn resolve_refs(v: &Value, results: &[Value]) -> Res<Value> {
    match v {
        Value::String(s) => match parse_ref(s) {
            Some((n, path)) => lookup(results, n, &path).map_err(|e| format!("reference \"{s}\": {e}")),
            None => Ok(v.clone()),
        },
        Value::Array(l) => l.iter().map(|x| resolve_refs(x, results)).collect::<Res<Vec<_>>>().map(Value::Array),
        Value::Object(m) => m.iter().map(|(k, x)| resolve_refs(x, results).map(|r| (k.clone(), r))).collect::<Res<serde_json::Map<_, _>>>().map(Value::Object),
        _ => Ok(v.clone()),
    }
}

fn lookup(results: &[Value], n: usize, path: &[&str]) -> Res<Value> {
    let mut cur = results
        .get(n)
        .ok_or_else(|| format!("operation {n} has not run yet (only operations before this one can be referenced; {} done so far)", results.len()))?;
    for seg in path {
        cur = match cur {
            Value::Object(m) => m.get(*seg),
            Value::Array(l) => seg.parse::<usize>().ok().and_then(|i| l.get(i)),
            _ => None,
        }
        .ok_or_else(|| format!("\"{}\" not found in the result of operation {n}: {cur}", path.join(".")))?;
    }
    Ok(cur.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whole_strings_of_the_exact_form_are_references() {
        assert_eq!(parse_ref("$0.id"), Some((0, vec!["id"])));
        assert_eq!(parse_ref("$12.ids.1"), Some((12, vec!["ids", "1"])));
        for literal in ["$0", "$0.", "$.id", "$a.id", "x $0.id", "$0.id is the id", "$0.id.", "$0..id", "cost: $0.id", "0.id", "$-1.id"] {
            assert_eq!(parse_ref(literal), None, "{literal}");
        }
    }

    #[test]
    fn references_are_resolved_at_any_depth_and_keep_their_type() {
        let results = vec![json!({"id": "a1", "ids": ["x", "y"], "n": 3})];
        let args = json!({"from": "$0.id", "list": ["$0.ids.1", {"k": "$0.n"}], "text": "see $0.id", "keep": 1});
        let out = resolve_refs(&args, &results).unwrap();
        assert_eq!(out, json!({"from": "a1", "list": ["y", {"k": 3}], "text": "see $0.id", "keep": 1}));
    }

    #[test]
    fn bad_references_say_why() {
        let results = vec![json!({"id": "a1"})];
        let e = resolve_refs(&json!("$1.id"), &results).unwrap_err();
        assert!(e.contains("has not run yet"), "{e}");
        let e = resolve_refs(&json!("$0.nope"), &results).unwrap_err();
        assert!(e.contains("not found in the result of operation 0"), "{e}");
        let e = resolve_refs(&json!("$0.id.deeper"), &results).unwrap_err();
        assert!(e.contains("not found"), "{e}");
    }

    fn add(text: &str) -> Value {
        json!({"tool": "add_shape", "args": {"geom": "rect", "text": text}})
    }

    fn failing() -> Value {
        json!({"tool": "delete_object", "args": {"id": "nope"}})
    }

    #[test]
    fn a_batch_is_one_undo_step_and_a_failed_atomic_one_leaves_no_trace() {
        let mut ed = Editor::new(inkboard_core::Document::default());
        // the application runs every tool inside `transact`: same here
        let r = ed.transact(|ed| batch(ed, &json!({"operations": [add("a"), add("b")]}))).unwrap();
        assert_eq!(r["applied"], 2);
        assert_eq!(ed.page().objects.len(), 2);
        let (doc, rev) = (ed.doc.clone(), ed.revision());
        let e = ed.transact(|ed| batch(ed, &json!({"operations": [add("c"), add("d"), failing()]}))).unwrap_err();
        assert!(e.contains("operation 2"), "{e}");
        assert_eq!((&ed.doc, ed.revision()), (&doc, rev));
        // without an enclosing transaction too
        assert!(batch(&mut ed, &json!({"operations": [add("c"), failing()]})).is_err());
        assert_eq!((&ed.doc, ed.revision()), (&doc, rev));
        assert!(ed.undo() && ed.page().objects.is_empty(), "the successful batch is one undo step");
        assert!(!ed.can_undo(), "the failed batches left no history entry");
    }

    #[test]
    fn a_non_atomic_batch_keeps_its_successes_as_one_undo_step() {
        let mut ed = Editor::new(inkboard_core::Document::default());
        let r = ed.transact(|ed| batch(ed, &json!({"atomic": false, "operations": [add("a"), add("b"), failing(), add("c")]}))).unwrap();
        assert_eq!((r["applied"].clone(), r["failed"]["index"].clone()), (json!(2), json!(2)));
        assert_eq!(ed.page().objects.len(), 2);
        assert!(ed.undo() && ed.page().objects.is_empty() && !ed.can_undo());
    }

    #[test]
    fn path_keys_are_found_at_any_depth() {
        assert!(has_path_key(&json!({"a": [{"b": {"source": "x"}}]})));
        assert!(has_path_key(&json!({"output": "x"})));
        assert!(!has_path_key(&json!({"text": "path", "paths": 1})));
    }
}
