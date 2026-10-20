<!-- Lot 2 (render): sections to paste into docs/MCP.md. -->

### Row for the "Tools" table

| `render` | **Picture of the board as an image** (PNG returned directly to the agent, nothing to export and read back). Whole content, a `region`, or the `ids` to frame; optional id badges (`show_ids`); `output` also writes the PNG under the root. Read-only. |

### Section "Seeing the board: `render`"

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

### Section "Tool results with an image" (convention, for developers)

A tool result is a JSON value, carried as is by the live protocol (one JSON line per response). The reserved top-level key `_image` (`{"mime": "image/png", "data_base64": "…"}`) marks a result that includes a picture: `inkboard-mcp` (`tools/call`) then answers with `content = [{"type": "image", "data": …, "mimeType": …}, {"type": "text", "text": <the JSON without _image>}]`. Every other result is still a single text block, exactly as before. Because the picture travels inside the JSON line, a live response can be several MiB long (the PNG is kept under 4 MiB, 5.5 MiB in base64).

### Line for "Limits"

- `render` measures text approximately and, in live mode, runs on the application's UI thread (a very large picture can briefly freeze the window); the application must answer within 30 seconds.
