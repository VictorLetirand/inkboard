<!-- Lot 4 (rich text): sections ready to paste into docs/MCP.md. Not edited there by the lot (see PLAN_MCP_V2.md, 2.3). -->

## Tools table — rows to add or replace

| Tool | Purpose |
|---|---|
| `add_shape` | `rect`, `ellipse`, `diamond`, `triangle`, `sticky`, `text`, with text (optionally [rich text](#rich-text)), colors, size. |
| `add_connector` | **Bound** arrow between two objects (`straight`, `elbow`, `curve`, arrowheads, label — optionally rich text). |
| `add_table` | Table of cells (columns fitted to the text); cells may be rich text. |
| `update_object` / `delete_object` | Modify (text — optionally rich text —, position, size, colors) or delete. |
| `format_text` | Bold, italic, underline, strike, color, highlight on text **already on the board**: a part of a shape's text, a connector's label or a table cell. |

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
