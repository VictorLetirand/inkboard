//! Tools of the MCP v2 plan (see `context/PLAN_MCP_V2.md`); the module is registered in `tools.rs`.

use super::Res;
use inkboard_core::Editor;
use serde_json::Value;

/// Tools exposed by this module.
pub(super) const NAMES: &[&str] = &[];
/// Those that do not modify the board.
pub(super) const READ_ONLY: &[&str] = &[];

/// Schemas for `tools/list`.
pub(super) fn specs() -> Vec<Value> {
    Vec::new()
}

/// Runs the tool when this module owns it, `None` otherwise.
pub(super) fn apply(_ed: &mut Editor, _name: &str, _a: &Value) -> Option<Res<Value>> {
    None
}
