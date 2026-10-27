# MCP server (`inkboard-mcp`)

*Version française : [fr/MCP.md](fr/MCP.md)*

Lets an agent (Claude, or any MCP client) **create and modify boards** by calling tools. Two modes, chosen automatically:

- **Live** (application open): the tool acts on the board **displayed** in the application. Each call is **one undo step** (Ctrl+Z),
  the created object is selected, `describe_board` reports the **user's selection** ("modify what I selected"), and nothing is saved
  to disk until the user (or `save_board`) decides so. If the board is empty, the view fits the content.
- **File** (application closed, or `path` of another file): the tool reads and writes the `.inkboard` file; if the application has this file open,
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
With `--root`, every path (relative or absolute) must be under that folder; `..` and links (including a link as the file itself) that lead outside it are refused. Only board files
are modified (`.inkboard`; the older `.inkboard.json` and `.tableau.json` names are still read and written under their own name, and `create_board` makes `.inkboard` files); `export` writes only `.inkboard`, `.inkboard.json`, `.drawio`, `.excalidraw`, `.canvas`, `.svg`, `.png`, `.pdf`, `.json`.

### Installed with the application

The installers put `inkboard-mcp` next to the application (Windows, per-user install: `%LOCALAPPDATA%\Inkboard\inkboard-mcp.exe`), so there is nothing to build: register that file in your client, as below.

### GitHub Copilot (VS Code)

*Not tested with Inkboard yet: written from the VS Code documentation.* VS Code starts local stdio servers from `.vscode/mcp.json` (workspace) or the user configuration. The root key is `servers` (not `mcpServers`):

```json
{
  "servers": {
    "inkboard": {
      "type": "stdio",
      "command": "C:\\Users\\YOU\\AppData\\Local\\Inkboard\\inkboard-mcp.exe",
      "args": ["--root", "D:\\boards"]
    }
  }
}
```

- Use an absolute path for `command`.
- Switch the Copilot Chat mode to **Agent**: MCP tools are not used in *Ask* mode, and each tool asks for approval.
- VS Code only uses a server's **tools**: resources and change notifications are ignored, so the agent must call `board_changes` to see what you changed.
- If the client does not display the image block of `render`, give `output` (a `.png` under the root): the picture is also written there.
- Microsoft 365 Copilot (Teams, Word…) cannot use this server: it only reaches **remote** MCP servers over HTTP, and Inkboard speaks stdio only.

### Claude Desktop and other clients

*Not tested.* The same command and arguments in the client's MCP configuration (for Claude Desktop, `mcpServers` in `claude_desktop_config.json`): `{"mcpServers": {"inkboard": {"command": "<path>\\inkboard-mcp.exe", "args": ["--root", "<folder>"]}}}`.

### In a container (file mode)

[`packaging/docker/mcp.Dockerfile`](../packaging/docker/mcp.Dockerfile) builds the server as a container image, for clients that start MCP servers through Docker and for server directories that build from a Dockerfile. There is no window and no running application in a container, so the server starts with `--no-live` and works on the `.inkboard` files of the mounted folder (`--root /data`):

```bash
docker build -f packaging/docker/mcp.Dockerfile -t inkboard-mcp .
docker run -i --rm -v "$PWD/boards:/data" inkboard-mcp
```

The server speaks MCP over standard input and output (hence `-i`). The image runs as a non-root user (uid 10001): the mounted folder must be writable by it. It was checked with a real MCP session (`initialize`, `tools/list` returning the 27 tools, then `create_board`, `add_shape` and `describe_board` on a file of the mounted folder, and a path outside `/data` refused). Live mode (editing the board that is open on screen) needs the desktop application and is not available in a container.

### Arrowheads

`add_connector` and `update_object` take `head_start` and `head_end` to choose the head of each end by name (`arrow` still sets both at once; a named head wins for its end). Original heads: `none`, `arrow`, `triangle`, `dot`, `bar`. Added heads, for UML and entity-relationship diagrams: `diamond` (composition), `diamond_open` (aggregation), `triangle_open` (inheritance, realization), `crow_foot` (many), `crow_foot_one` (one or many), `crow_foot_zero` (zero or many), `one_only` (exactly one), `zero_or_one`. `describe_board` with `detail: "full"` reports them in `heads`.

The added heads are stored in separate optional fields of the connector (`head_start_ext`, `head_end_ext`), next to the original `head_start` / `head_end`, which keep the closest original head (a diamond is a `dot`, a hollow triangle a `triangle`, a crow's foot an `arrow`, `one_only` a `bar`). A version that does not know the added heads ignores the new fields and draws that head, so a board that uses them still opens, and a board that does not is written exactly as before. In a live session, a guest on an older version who edits such a connector sends it back without the added heads, which then show their original head for everybody. draw.io and Excalidraw files use their own symbols (draw.io: `diamond`, `ERmany`, `ERoneToMany`, `ERzeroToMany`, `ERmandOne`, `ERzeroToOne`, with `endFill=0` for hollow ones).

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
| `describe_board` | Pages and objects (id, type, text, position, connector links); in live mode, also `selection`. `detail: "full"` adds [styles and fonts](#reading-styles). Also reads draw.io, Excalidraw, JSON Canvas, Mermaid. |
| `find_objects` | Finds objects by text, regex, type or area, without reading the whole board (read-only). |
| `render` | **Picture of the board as an image** (PNG returned directly to the agent, nothing to export and read back). Whole content, a `region`, or the `ids` to frame; optional id badges (`show_ids`); `output` also writes the PNG under the root. Read-only. |
| `add_shape` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, with text (optionally [rich text](#rich-text)), colors, size. |
| `add_connector` | **Bound** arrow between two objects (`straight`, `elbow`, `curve`, arrowheads, label — optionally rich text). |
| `add_table` | Table of cells (columns fitted to the text); cells may be rich text. |
| `add_code_block` | Colored code block. |
| `add_formula` | LaTeX formula typeset as in TeX (clear error if unsupported). |
| `add_image` | PNG/JPEG/GIF/WebP/SVG image (15 MiB at most) from a file under the root (`source`) or in base64; natural size reduced to 600, or `w`/`h`. The type is recognized from the content, never from the extension. |
| `search_icons` | Searches the built-in icon library (about 28,800 icons: AWS, Azure, Google Cloud, Cisco, Kubernetes, brand logos, UI symbols…) by keywords (`query`, English names, case-insensitive, every word must match); optional `group`, `set`, `limit` (default 20, at most 100). Returns `{count, truncated, icons: [{id, name, set, group, mono}]}`. Read-only. |
| `list_icon_groups` | The groups of the icon library with their sets and icon counts (`{total, groups: [{group, sets: [{set, icons}]}]}`), to know what to search for. Read-only. |
| `add_icon` | Places an icon (`icon` = an `id` from `search_icons`) as an image object that `add_connector` can attach to. `size` (longest side, default 64, 8 to 2000) or `w`/`h` (ratio kept when only one is given); `stroke` tints monochrome icons; `label` adds a text centered under the icon, grouped with it (`markdown: true` for [rich text](#rich-text)). Returns `{id, x, y, w, h, icon, label_id?, warnings?}`. |
| `import_mermaid` | Mermaid diagram (flowchart, state, sequence, class, entity-relationship) with automatic layout; optional dedicated page. A `subgraph` becomes a dashed frame with its title; its content is laid out on its own, so frames never overlap, and its `direction` applies when no link joins it to the outside. A link written to a subgraph (`B --> Back`) is attached to that frame. A `;` inside a quoted label (`A["a; b"]`) is kept in the label. A class or an entity becomes a table (name, attributes, methods; or type, name, keys); inheritance and realization end in a hollow triangle, composition and aggregation in a diamond, and an entity link ends in crow's foot symbols and spells out its cardinalities. Not supported: pie, mindmap, gantt (an explicit error says so). |
| `update_object` | Modify text ([rich text](#rich-text) optional), position, size, colors, [line style, font, lock, rotation](#update_object), a connector's route / arrowheads / ends, or one table cell. |
| `delete_object` | Deletes one object (`id`) or several (`ids`). |
| `move_objects` | Moves objects together (relative or to a position); bound connectors follow. |
| `arrange_objects` | Aligns, distributes, stacks (front/back), groups, ungroups, locks, unlocks. |
| `duplicate_objects` | Copies objects (one or several copies, with an offset); connectors between copied objects are kept. |
| `format_text` | Bold, italic, underline, strike, color, highlight on text **already on the board**: a part of a shape's text, a connector's label or a table cell. |
| `auto_layout` | Rearranges a graph of connected shapes (TB, LR, BT, RL). |
| `add_page` | New page. |
| `manage_pages` | Lists, renames or deletes pages (`add_page` creates them). |
| `batch` | **Several tool calls in one request** (1 to 200), run in order on one board, with references to earlier results (`"$0.id"`), all-or-nothing by default, one undo step. |
| `export` | `.svg`, `.png`, `.pdf`, `.drawio`, `.excalidraw`, `.canvas`, `.inkboard` according to the extension. The missing folders of `output` are created (under the root; `render` does the same for its `output`). For a `.png`, `width` (16 to 8000) gives the width in pixels whatever the size of the diagram, so that several diagrams get the same width; or `scale` (0.1 to 8, default 2) gives the pixels per board unit. Not both. |

Without `x`/`y`, a new object is placed below the existing content. The returned ids are used to connect or modify objects.
Errors are returned as a tool result (`isError`) with an English message the agent can act on.

## Icons

The icon library of the application (the palette) is also available to the agent, offline: the pack is embedded in `inkboard-mcp`, which makes the
standalone server about 8 MB larger. Typical use: `list_icon_groups` once to see the groups, `search_icons` (`{"query": "lambda", "group": "AWS"}`),
then `add_icon` with the returned `id`, then `add_connector` between the returned image ids.

- **Icon id**: `<set id>/<icon name>`, for example `drawio-aws4/Lambda` or `simple-icons/GitHub`. A few sets contain two icons with the same name; the
  second one gets the suffix `#2` (`simple-icons/Backstage#2`). Ids come from the pack: copy them from `search_icons`, do not build them.
- **`group` / `set`** (search): `group` is a group name as listed by `list_icon_groups` (case-insensitive); `set` is the exact title or id of a set. An
  unknown value is an error that lists the valid groups, or the closest sets.
- **Colors**: icons with `mono: true` (general icons, diagram shapes, rack equipment…) are single-color and take the object's `stroke` color; without
  `stroke`, the theme's text color (so they stay readable on a dark board). Colored icons (clouds, brands) keep their own colors: a `stroke` given for
  them is ignored, and the result says so in `warnings`.
- **Placement**: like the palette, the image has no outline and no fill; the same icon placed several times is stored once in the file. Without `x`/`y`,
  the icon goes below the existing content. With `label`, the text box is centered under the icon (6 units below) and grouped with it, so the pair moves
  together; the returned `id` is the image, which is the object to connect.
- **Errors** (the board is left unchanged): empty or too long `query` (200 characters at most), `limit` outside 1–100, unknown `group` / `set`, `size` /
  `w` / `h` outside 8–2000, `size` together with `w` / `h`, invalid `stroke`, `label` over 500 characters, and an unknown icon: the message gives up to 5
  close ids (`icon not found: "ec2" ; did you mean: … ?`).
- The search matches the icon name, its set title and its group, and is case-insensitive (not accent-insensitive: the names are in English).
- In file mode, the two read-only tools still need the `path` of a board (it is loaded, never rewritten); in live mode they need nothing.

## Seeing the board: `render`

`render` returns a PNG of the page as an MCP **image** content block, followed by a text block with a JSON description of the picture. It never modifies the board, the file or the undo history, in live mode as in file mode.

| Parameter | Meaning |
|---|---|
| `path`, `page` | As for the other tools (`path` optional in live mode; `page` defaults to the first). |
| `region` | `{x, y, w, h}` in board units (the same units as `describe_board`): shows exactly this rectangle. `w` and `h` must be greater than 0. |
| `ids` | Ids of objects to frame: the picture shows their bounding box plus `padding`, and still draws what is around them. Unknown ids are listed in the error. Not combinable with `region` (error "give either region or ids"). |
| `padding` | Margin around the content or the framed ids, in board units (default 24). Not added to an explicit `region`. |
| `max_px` | Longest side of the picture in pixels (default 1400, from 16 to 3000). A small region is enlarged 4 times at most. |
| `show_ids` | `true` draws a small white badge with the id of every visible object on top of it (top-left corner; middle of an arrow; pen strokes have none), so that what the agent sees can be linked to the ids that `update_object` or `add_connector` expect. Only the picture is affected, the board is not. |
| `output` | Also writes the PNG to this file, for clients that do not display images. Must end with `.png` and be under `--root` (like every path); a board file can never be overwritten this way. |

The text block holds `width_px`, `height_px`, `region` (the rectangle actually shown), `scale` (pixels per board unit), `ids_shown` (badges drawn; 0 without `show_ids`) and `output` when given. A picture whose PNG would exceed 4 MiB is automatically produced smaller; the block then also holds `max_px_used` and a `note`. An empty page is an error (`the page is empty: nothing to render`). Text is measured approximately (as in `export`), so the picture can differ a little from what the application shows.

### Tool results with an image (convention, for developers)

A tool result is a JSON value, carried as is by the live protocol (one JSON line per response). The reserved top-level key `_image` (`{"mime": "image/png", "data_base64": "…"}`) marks a result that includes a picture: `inkboard-mcp` (`tools/call`) then answers with `content = [{"type": "image", "data": …, "mimeType": …}, {"type": "text", "text": <the JSON without _image>}]`. Every other result is still a single text block, exactly as before. Because the picture travels inside the JSON line, a live response can be several MiB long (the PNG is kept under 4 MiB, 5.5 MiB in base64).

## Reading styles

`describe_board` takes `detail`: `"brief"` (default, the output is unchanged) or `"full"`. With `full`, each object also has:

| Field | Content |
|---|---|
| `style` | `{stroke, fill, width, dash, opacity, roundness}`: colors are `"#rrggbb"`, `"ink"` or (`fill`) `null`; `dash` is `solid`, `dashed` or `dotted`. |
| `font` | `{family, size, align}` for shapes and connector labels (`sans`, `hand`, `serif`, `mono`; `left`, `center`, `right`); `{family, size}` for tables. |
| `z` | Stacking index in the page: the higher, the closer to the front. |
| `group` | Group id, when the object is grouped. |
| `locked` | `true`, only when the object is locked. |
| `rotation` | Degrees, only for a rotated shape or image. |
| connectors | `route` (`straight`, `elbow`, `curve`), `arrow` (`end`, `start`, `both`, `none`), `stroke`; `heads` only for arrowheads other than the plain arrow (`triangle`, `dot`, `bar` and the [added heads](#arrowheads)). |
| tables | `cells`: the plain text of each cell, row by row. |

`selection` is unchanged.

## `find_objects`

Read-only. Looks at one page (`page`); all the filters you give must match.

| Parameter | Meaning |
|---|---|
| `text` | Case-insensitive substring of a shape's text, a connector's label, a table's cells or a formula's LaTeX. |
| `regex` | Regular expression (Rust syntax, 500 characters at most, matched anywhere; `(?i)` for case-insensitive). Alternative to `text`. |
| `type` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, `connector`, `table`, `image`, `formula`, `pen`. |
| `in_region` | `{x, y, w, h}`: objects whose bounding box **overlaps** the rectangle. |
| `limit` | 1 to 500, default 50. |

Result: `{"count": n, "truncated": bool, "objects": [...]}` where `count` is the number of **all** matches, `truncated` tells that there are more than `limit`, and `objects` has the same summaries as `describe_board` (brief).
An empty or invalid `text`/`regex`, both given, an unknown `type`, a bad `in_region` or `limit` are errors. The regex engine runs in linear time and is size-limited.

## `update_object`

The existing parameters (`text`, `markdown`, `x`, `y`, `w`, `h`, `fill`, `stroke`) are unchanged. New ones:

| Parameter | Values | Applies to |
|---|---|---|
| `stroke_width` | 0 to 100 (0: no outline) | any object |
| `dash` | `solid`, `dashed`, `dotted` | any object |
| `opacity`, `roundness` | 0 to 1 | any object |
| `font_size` | 4 to 400 | shapes, connector labels, tables |
| `font_family` | `sans`, `hand`, `serif`, `mono` | shapes, connector labels, tables |
| `align` | `left`, `center`, `right` | shapes, connector labels |
| `locked` | `true` / `false` | any object |
| `rotation` | -360 to 360, **absolute** angle in degrees (0 resets), around the center | shapes, images |
| `route` | `straight`, `elbow`, `curve` | connectors |
| `arrow` | `end`, `start`, `both`, `none` | connectors |
| `head_start`, `head_end` | an [arrowhead](#arrowheads) name; replaces what `arrow` gave that end | connectors |
| `from`, `to` | id of a shape, table or image: reconnects that end (same rules as `add_connector`) | connectors |
| `head_scale` | 0.5 to 4, size of the arrowheads (1: default; the panel's S / M / L / XL are 0.6 / 1 / 1.6 / 2.4); also on `add_connector` | connectors |
| `shadow` | `true` / `false`: drop shadow; also on `add_shape` | shapes, images, tables, connectors |
| `recolor` | replaces colors of an SVG image: `{"#e34f26": "#0055ff"}` or `[["#e34f26", "#0055ff"]]`; replaces the previous list, `{}` restores the original colors (at most 256 pairs). `describe_board` with `detail: "full"` lists the colors found in the file | SVG images (not monochrome icons or formulas) |
| `cell` + `text` | `[row, column]` from 0: the new text of that table cell (`markdown` works as everywhere); the column widens if needed | tables |

- **All or nothing**: every parameter is validated first; an unknown or out-of-range value is an error that names the parameter and the allowed values, and nothing is modified.
- A parameter that does not suit the object (`rotation` on a pen, `route` on a rectangle…) is **ignored** and listed in `avertissements` (`warnings` stays reserved for Markdown problems, see Rich text). `cell` on something that is not a table is an error.
- Font parameters on a connector apply to its label; without a label they are reported (give `text` too).
- **Locked objects are not protected from tools**: `update_object`, `move_objects`, `duplicate_objects` (the copy stays locked) and `delete_object` act on them. Only the alignment and distribution of `arrange_objects` skip them (and objects on a locked layer).

```json
{"name": "update_object", "arguments": {"id": "…", "dash": "dashed", "opacity": 0.6, "font_family": "mono", "rotation": 15}}
{"name": "update_object", "arguments": {"id": "…", "cell": [1, 2], "text": "**42**", "markdown": true}}
```

## `move_objects`

`ids` (1 to 2000) and **either** `dx`, `dy` (relative; one of them is enough) **or** `x`, `y` (new top-left corner of the **bounding box of the whole batch**; for one object it is `update_object`'s `x`/`y`). Giving both forms, or none, is an error.
Connectors bound to the moved objects follow; a connector moved alone only moves its free ends. Result: `{"moved": n, "dx": …, "dy": …}`.

## `arrange_objects`

`ids` (1 to 2000) and `action`:

| `action` | Effect |
|---|---|
| `align_left`, `align_center_x`, `align_right`, `align_top`, `align_center_y`, `align_bottom` | Aligns on the bounding box of the objects. At least 2 unlocked objects. |
| `distribute_horizontal`, `distribute_vertical` | Equal gaps between the two outermost objects. At least 3 unlocked objects. |
| `bring_to_front`, `bring_forward`, `send_backward`, `send_to_back` | Stacking order (`z`). |
| `group` | At least 2 objects share a new group; the result has `group`. |
| `ungroup` | Releases the listed objects (result: `ungrouped`). |
| `lock`, `unlock` | Locks / unlocks the objects. |

Groups are **not expanded**: list every member to move, align or ungroup a whole group.
Locked objects (and objects on a locked layer) are skipped by `align_*` and `distribute_*` and listed in `skipped_locked`. The engine alone does nothing when there are too few unlocked objects (fewer than 2 to align, 3 to distribute); the tool returns an error instead.

## `manage_pages`

| `action` | Meaning |
|---|---|
| `list` | `{"pages": [{"number", "name", "objects"}]}`. |
| `rename` | `page` (number from 1, also written as text such as `"2"`, or name) and `name` (not empty, 120 characters at most, trimmed). `warnings` says if another page has the same name. |
| `delete` | `page`; also deletes its objects (`objects_removed`). The last page of a board cannot be deleted. |

`rename` and `delete` require `page` (the first page is never assumed). `add_page` creates pages.

## `delete_object`

`id` (one object, result `{"deleted": "<id>"}`) **or** `ids` (1 to 2000, result `{"deleted": ["<id>", …]}`); both together, or none, is an error. If one of the `ids` does not exist, nothing is deleted. Connectors bound to deleted objects stay, detached.

## `duplicate_objects`

`ids`, `dx`, `dy` (default 40, 40), `count` (1 to 50, default 1; at most 10 000 new objects in all). Copy *k* is shifted by *k* × (`dx`, `dy`) from the originals.
Connectors **between** copied objects are copied and joined to the copies; a connector joined to an object that is **not** copied becomes free at that end in the copy. Groups are copied as new groups. The copies are added on top.
Result: `{"created": n, "sources": [...], "ids": [...]}` — `sources` are the copied ids in page order; `ids` are the new ones: copy 1 in the order of `sources`, then copy 2…

```json
{"name": "duplicate_objects", "arguments": {"ids": ["…", "…"], "dx": 0, "dy": 120, "count": 3}}
```

## Rich text

By default, `text` is plain: `**x**` stays `**x**`. Pass **`markdown: true`** to `add_shape` (`text`), `update_object` (`text` of a shape or a connector),
`add_table` (every cell) or `add_connector` (`label`) to write formatted text.

| Write | Result |
|---|---|
| `**bold**` | bold |
| `*italic*` or `_italic_` | italic |
| `__underline__` | underlined |
| `~~strike~~` | struck through |
| `{color=#e03131}text{/}` | text color (`#rrggbb`, `#rgb`, or `ink`, the theme's text color) |
| `{bg=#ffec99}text{/}` | highlight behind the text |
| `{color=#e03131 bg=ink}text{/}` | both in one tag |
| `\*` `\_` `\~` `\{` `\}` `\\` | the character itself |

- Everything combines and nests: `{color=#e03131}**warning:** *check* the {bg=#ffec99}input{/}{/}`. `{/}` closes the last open tag; the other markers close on their second occurrence. Layers may overlap (`**a *b** c*`).
- Line breaks (`\n`) are unchanged.
- Escape a literal `*`, `_`, `~`, `{`, `}` or `\` with a backslash, for example in a product of numbers (`2 \* 3`) or an identifier (`__init__` is underlined: write `\_\_init\_\_`). A single `_` inside a word (`snake_case_name`) stays plain; `~` alone and a `*` or `_` that closes nothing are kept as they are.
- **Invalid markup is never an error and never loses text**: an unclosed marker, an empty pair (`****`), a `{/}` that closes nothing, an invalid color (`{color=red}`: only `#rrggbb`, `#rgb` and `ink`) or an unknown attribute stay in the text as written, and the result lists them in a `warnings` array of messages (the field is absent when everything was understood). In a table, each message starts with `cell [row, column]:` (from 0).
- The shape is fitted after the change as before; bold counts in the text width.

```json
{"name": "add_shape", "arguments": {"geom": "sticky", "markdown": true,
  "text": "**Deploy**\n{color=#e03131}blocked{/} until {bg=#ffec99}review{/}"}}
```

## `format_text`

Changes the formatting of text that is already on the board, like selecting it in the application.

| Parameter | Meaning |
|---|---|
| `id` (required) | A shape, a connector with a label, or a table (with `cell`). |
| `match` | Substring to format (case-sensitive): the first occurrence, or every one with `all: true`. |
| `range` | `[start, end)`: character indices of the plain text, `start` included, `end` excluded (characters, not bytes). |
| `cell` | `[row, column]`, from 0: required for a table, invalid otherwise. |
| `bold`, `italic`, `underline`, `strike` | `true` applies, `false` removes (never a toggle). |
| `color`, `highlight` | `"#rrggbb"` / `"ink"` sets, `null` removes. |
| `clear` | `true` removes all formatting of the part first; the other options then apply (`clear` + `bold` = reset, then bold). |

Without `match` and `range`, the whole text (of the cell) is formatted. `match` with `range`, `all` without `match`, an empty or out-of-range selection, an invalid color, a call that asks for nothing,
a `match` that is not found (`text not found: "…" (the text is "…")`, with the beginning of the real text) and an object without text are errors; the board is left untouched.

Result: `{"id": "…", "changed": n}` — `n` is the number of characters whose formatting actually changed (`0`: already formatted that way).

```json
{"name": "format_text", "arguments": {"id": "…", "match": "blocked", "all": true, "bold": true, "color": "#e03131"}}
{"name": "format_text", "arguments": {"id": "…", "cell": [0, 1], "highlight": "#ffec99"}}
```

## Reading formatted text

`describe_board` (and `board_changes`) keep their output **as is** for plain text. Only when some text carries formatting, the object also has:

- `markdown` (shapes, and connectors for their label): the text in the syntax above, ready to give back to a `markdown: true` call; `text` / `label` still hold the plain text;
- `cells_markdown` (tables): the rows × columns grid of every cell in the same syntax, only if at least one cell is formatted.

```json
{"id": "…", "type": "rect", "text": "Deploy blocked", "markdown": "**Deploy** {color=#e03131}blocked{/}", "x": 0, "y": 0, "w": 180, "h": 90}
```

Limits: formatting a table cell is done with `add_table` (`markdown: true`) or `format_text` (`cell`); `update_object` still does not change a cell. Fonts and sizes are not part of rich text.

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

Any tool of the server can be used in a batch, including the ones added by later versions. Names are given as the server knows them (`add_shape`); the `inkboard_` prefix that some clients show (`inkboard_add_shape`) is accepted too. **Not allowed**: `batch` (no nesting), `create_board`, `save_board`, `board_changes`, `export` and `render` (they write elsewhere than on the board or would confuse the change tracking). The error is `tool X cannot be used inside a batch`.

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

## Resources and notifications

The server declares the MCP `resources` capability (with `subscribe` and `listChanged`):

- `inkboard://live`: the board displayed in the application (same content as `describe_board`, with the selection) — present only when the application is open;
- `file:///…/*.inkboard` (also `*.inkboard.json`, `*.tableau.json`): the boards under the `--root` folder (3 folder levels, 300 files at most).

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
- `render` measures text approximately and, in live mode, runs on the application's UI thread (a very large picture can briefly freeze the window); the application must answer within 30 seconds.
- A `batch` holds at most 200 operations; its results are all returned in one answer (a very large `describe_board` inside a batch makes a large answer). In an atomic failure, the quoted partial results are cut after 8,000 characters. Images added by an operation that is then rolled back may stay in memory until the next save of the application (the file is never written on failure).
- Several agents writing to the same file at once (file mode): the last one wins.
