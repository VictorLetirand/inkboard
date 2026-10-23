<!-- Lot 5 (batch): sections ready to paste into docs/MCP.md. Not edited there by the lot (see PLAN_MCP_V2.md, 2.3). -->

## Tools table — row to add

| Tool | Purpose |
|---|---|
| `batch` | **Several tool calls in one request** (1 to 200), run in order on one board, with references to earlier results (`"$0.id"`), all-or-nothing by default, one undo step. |

## Many operations in one call: `batch`

`batch` runs a list of operations, each one `{"tool": "<name>", "args": {…}}`, with the arguments the tool normally takes. It is the fast way to build or rework a board: no round trip per object, and the ids returned by one operation can be used by the next ones.

```json
{"path": "plan.inkboard", "operations": [
  {"tool": "add_shape", "args": {"geom": "rect", "text": "Client"}},
  {"tool": "add_shape", "args": {"geom": "rect", "text": "Server"}},
  {"tool": "add_connector", "args": {"from": "$0.id", "to": "$1.id", "label": "request"}}
]}
```

| Parameter | Meaning |
|---|---|
| `operations` | **Required.** 1 to 200 operations, run in order. |
| `atomic` | Default `true`: all or nothing (see below). `false`: keep what succeeded before the first error. |
| `path`, `page` | As for the other tools. They apply to **every** operation; an operation can name another `page` in its own `args`. An operation without `page` always acts on the batch-level `page` (default: the first page), whatever the previous operations did (even `add_page`). |

Any tool of the server can be used in a batch, including the ones added by later versions. **Not allowed**: `batch` (no nesting), `create_board`, `save_board`, `board_changes`, `export` and `render` (they write elsewhere than on the board or would confuse the change tracking). The error is `tool X cannot be used inside a batch`.

### References

A string that is **exactly** `"$N.path"` is replaced, before the operation runs, by a value of the **result** of operation `N` (counted from 0, and `N` must be an earlier operation). `path` is made of keys and list indexes separated by dots: `"$0.id"`, `"$3.ids.1"` (second id created by an `import_mermaid` at index 3). The value keeps its type (string, number, list…), and references are found anywhere in `args`, also inside lists and objects.

- Only the **whole string** is a reference: a `text` such as `"see $0.id"` stays literal, and so does a string that does not look like `$N.key` (`"$5"`, `"cost $0.id."`). Beware of a text that is exactly like a reference, such as `"$3.99"`: it would be taken for one (write `"US$3.99"` or `"$ 3.99"`).
- A reference to an operation that has not run yet, or to a path the result does not have, is an error that names the operation and the reference.

### Errors and atomicity

- **`atomic: true`** (default). At the first error, every operation already done is undone: the board, the file and the undo history are exactly as before. The call fails (`isError`) with the **index** and the tool of the failing operation, its error, and the **results obtained before the rollback** (so you can see what was going well). In the application, the whole batch otherwise counts as **one** undo step (Ctrl+Z).
- **`atomic: false`**. The batch stops at the first error and **keeps** what succeeded (one undo step for the whole). The answer is a normal result: `{"applied": 4, "failed": {"index": 4, "tool": "…", "error": "…"}, "results": [...]}`. The failing operation itself leaves nothing behind.
- Success: `{"applied": n, "results": [<result of each operation>]}`.
- Requests that are malformed (not a list, empty, more than 200 operations, unknown key in an operation, forbidden tool, forbidden key) are refused **before** anything runs.

### Security

The confinement to `--root` is applied to the batch-level `path`. An operation **cannot carry** a key named `path`, `output` or `source` — neither next to `tool`, nor in `args`, at any depth: the error is `paths cannot be set inside a batch operation; use the batch-level path`. To add an image, use `data_base64` (the 15 MiB limit applies as usual). Changes made by a batch are the agent's own: `board_changes` does not report them as changes by the user.

### Line for "Limits"

- A `batch` holds at most 200 operations; its results are all returned in one answer (a very large `describe_board` inside a batch makes a large answer). In an atomic failure, the quoted partial results are cut after 8,000 characters. Images added by an operation that is then rolled back may stay in memory until the next save of the application (the file is never written on failure).
