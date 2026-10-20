<!-- Lot 1 (icons): rows for the "Tools" table of docs/MCP.md, then a section to paste after it. -->

| `search_icons` | Searches the built-in icon library (about 28,800 icons: AWS, Azure, Google Cloud, Cisco, Kubernetes, brand logos, UI symbols…) by keywords (`query`, English names, case-insensitive, every word must match); optional `group`, `set`, `limit` (default 20, at most 100). Returns `{count, truncated, icons: [{id, name, set, group, mono}]}`. Read-only. |
| `list_icon_groups` | The groups of the icon library with their sets and icon counts (`{total, groups: [{group, sets: [{set, icons}]}]}`), to know what to search for. Read-only. |
| `add_icon` | Places an icon (`icon` = an `id` from `search_icons`) as an image object that `add_connector` can attach to. `size` (longest side, default 64, 8 to 2000) or `w`/`h` (ratio kept when only one is given); `stroke` tints monochrome icons; `label` adds a text centered under the icon, grouped with it. Returns `{id, x, y, w, h, icon, label_id?, warnings?}`. |

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
