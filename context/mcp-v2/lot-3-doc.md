<!-- Lot 3 (reading styles, editing): sections ready to paste into docs/MCP.md. Not edited there by the lot (see PLAN_MCP_V2.md, 2.3). -->

## Tools table — rows to add or replace

| Tool | Purpose |
|---|---|
| `describe_board` | Pages and objects (id, type, text, position, connector links); in live mode, also `selection`. `detail: "full"` adds [styles and fonts](#reading-styles). Also reads draw.io, Excalidraw, JSON Canvas, Mermaid. |
| `find_objects` | Finds objects by text, regex, type or area, without reading the whole board (read-only). |
| `update_object` | Modify text, position, size, colors, [line style, font, lock, rotation](#update_object), a connector's route / arrowheads / ends, or one table cell. |
| `delete_object` | Deletes one object (`id`) or several (`ids`). |
| `move_objects` | Moves objects together (relative or to a position); bound connectors follow. |
| `arrange_objects` | Aligns, distributes, stacks (front/back), groups, ungroups, locks, unlocks. |
| `duplicate_objects` | Copies objects (one or several copies, with an offset); connectors between copied objects are kept. |
| `manage_pages` | Lists, renames or deletes pages (`add_page` creates them). |

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
| connectors | `route` (`straight`, `elbow`, `curve`), `arrow` (`end`, `start`, `both`, `none`), `stroke`; `heads` only for arrowheads other than the plain arrow (`triangle`, `dot`, `bar`). |
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
| `from`, `to` | id of a shape, table or image: reconnects that end (same rules as `add_connector`) | connectors |
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
| `rename` | `page` (number from 1 or name) and `name` (not empty, 120 characters at most, trimmed). `warnings` says if another page has the same name. |
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
