# MCP server (`inkboard-mcp`)

*Version française : [fr/MCP.md](fr/MCP.md)*

Lets an agent (Claude, or any MCP client) **create and modify boards** by calling tools. Two modes, chosen automatically:

- **Live** (application open): the tool acts on the board **displayed** in the application. Each call is **one undo step** (Ctrl+Z),
  the created object is selected, `describe_board` reports the **user's selection** ("modify what I selected"), and nothing is saved
  to disk until the user (or `save_board`) decides so. If the board is empty, the view fits the content.
- **File** (application closed, or `path` of another file): the tool reads and writes the `.inkboard.json` file; if the application has this file open,
  it **reloads it on its own** within half a second (unless there are unsaved local changes: it warns and replaces nothing).

In live mode, `path` is optional; if given and it designates the displayed file, the tool acts live; if it designates another file, it acts on that file.
Without `path` and without an open application, the tool answers with a clear error.

## Connecting the server

```bash
cargo build --release -p inkboard-mcp
claude mcp add inkboard -- /path/to/inkboard/target/release/inkboard-mcp --root /path/to/boards
```

For another client the configuration is the same: command `inkboard-mcp`, arguments `--root <folder>`, stdio transport.
**Always use `--root`**: without it the agent can read and write any file your account can reach.
With `--root`, every path (relative or absolute) must be under that folder; `..` and links (including a link as the file itself) that lead outside it are refused. Only `.inkboard.json` files
are modified; `export` writes only `.inkboard.json`, `.drawio`, `.excalidraw`, `.canvas`, `.svg`, `.png`, `.pdf`, `.json`.

## How live mode works

The application opens a local server on `127.0.0.1` (ephemeral port, never exposed to the network) and publishes its port and a random **secret token** in
`live.json` in its data folder (`%APPDATA%\Inkboard` on Windows, `~/Library/Application Support/Inkboard` on macOS, `~/.local/share/Inkboard` on Linux; deleted on exit). `inkboard-mcp` reads this file, forwards the call with the token, and the application runs the tool on its UI
thread. A request without a valid token is refused without running anything. Paths (`path`, `output`) are resolved and confined to `--root` by `inkboard-mcp`
before forwarding. Options: `--no-live` disables live mode; `--live-file` changes the discovery file.

## Tools

| Tool | Purpose |
|---|---|
| `create_board` | Creates an empty board (refuses to overwrite an existing file). |
| `save_board` | Live mode: saves the displayed board (it must already have a file). |
| `board_changes` | What the **user** changed since your last look (`describe_board` / `board_changes`): objects added, modified, deleted. Your own changes are not reported. |
| `add_image` | PNG/JPEG/GIF/WebP/SVG image (15 MiB at most) from a file under the root (`source`) or in base64; natural size reduced to 600, or `w`/`h`. The type is recognized from the content, never from the extension. |
| `describe_board` | Pages and objects (id, type, text, position, connector links); in live mode, also `selection`. Also reads draw.io, Excalidraw, JSON Canvas, Mermaid. |
| `add_shape` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, with text, colors, size. |
| `add_connector` | **Bound** arrow between two objects (`straight`, `elbow`, `curve`, arrowheads, label). |
| `add_table` | Table of cells (columns fitted to the text). |
| `add_code_block` | Colored code block. |
| `add_formula` | LaTeX formula typeset as in TeX (clear error if unsupported). |
| `import_mermaid` | Mermaid diagram (flowchart, state, sequence) with automatic layout; optional dedicated page. |
| `update_object` / `delete_object` | Modify (text, position, size, colors) or delete. |
| `auto_layout` | Rearranges a graph of connected shapes (TB, LR, BT, RL). |
| `add_page` | New page. |
| `export` | `.svg`, `.png`, `.pdf`, `.drawio`, `.excalidraw`, `.canvas`, `.inkboard.json` according to the extension. |

Without `x`/`y`, a new object is placed below the existing content. The returned ids are used to connect or modify objects.
Errors are returned as a tool result (`isError`) with an English message the agent can act on.

## Resources and notifications

The server declares the MCP `resources` capability (with `subscribe` and `listChanged`):

- `inkboard://live`: the board displayed in the application (same content as `describe_board`, with the selection) — present only when the application is open;
- `file:///…/*.inkboard.json`: the boards under the `--root` folder (3 folder levels, 300 files at most).

`resources/subscribe` on one of them: the server sends `notifications/resources/updated` when the content changes (file modified, or — for the displayed
board — a user modification **or selection change**), and `notifications/resources/list_changed` when the list changes. Change is
detected by polling every 600 ms. The first observation after subscribing serves as the baseline (no notification for the state already read).

**Depending on the client**, these notifications are not necessarily shown to the model: a client that does not handle subscriptions ignores them. That is why the
`board_changes` tool exists: the agent can ask by itself what the user changed, whatever the client.

## Limits

- Several application instances: the last one launched publishes the discovery file (the others cannot be driven).
- After an application crash, a stale discovery file can remain; `inkboard-mcp` then falls back to file mode.
- In live mode, the agent changes the displayed page when it targets another one (`page`), and the user's selection is replaced by the created objects.
- Pen strokes and image content cannot be read (only their presence, size and position).
- `board_changes` tracking is disabled beyond 20,000 objects (linear cost on every call).
- Resource notifications rely on polling (600 ms) and do not carry the detail of the change: the agent re-reads the resource or calls `board_changes`.
- The MCP protocol is implemented by hand (`initialize`, `ping`, `tools/list`, `tools/call`, `resources/*`); no prompts.
- Tested end to end with a client written for the purpose (`crates/inkboard-mcp/tests/session.rs`, `crates/inkboard-app/src/tests.rs`) **and with Claude Code 2.1.258**
  (file mode, then live mode against the real application: adding a rectangle, `describe_board` with `live: true` and selection). Resources, subscriptions and notifications are tested with the real binary and with the application (automated tests) but **not with a real MCP client that uses them** (Claude Code was used for the tools; what it does with notifications has not been verified). Not tested with Claude Desktop.
- Several agents writing to the same file at once (file mode): the last one wins.
