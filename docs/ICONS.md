# Icon library

*Version française : [fr/ICONS.md](fr/ICONS.md)*

Inkboard ships with about **28,800 icons and shapes** in 217 sets, available offline from the **Icon library** button of the toolbar (or *Insert → Icon library…*). Search by name, or filter by group and set; a click drops the icon at the center of the view. Icons are ordinary SVG image objects: move, resize, rotate, copy, export to SVG / PNG / PDF / draw.io / Excalidraw like any other object.

| Group | What it contains | Source |
|---|---|---|
| **AWS** | architecture icons (current and four earlier generations), 3D icons, groups | draw.io |
| **Microsoft Azure** | Azure 2021 icons, Cloud and Enterprise, Dynamics 365, Active Directory | draw.io |
| **Microsoft Office** | servers, devices, users, concepts, clouds… | draw.io |
| **Google Cloud** | product icons and categories | draw.io |
| **Cisco** | network equipment, SAFE architecture, rack equipment | draw.io |
| **IBM** | IBM Cloud and product icons | draw.io |
| **Cloud and IT** | Kubernetes, Alibaba Cloud, Atlassian, Salesforce, SAP, OpenStack, VMware, Veeam, Citrix, networking, rack equipment, web logos | draw.io |
| **Diagram shapes** | basic shapes, flowchart, BPMN, arrows, enterprise integration patterns, sitemap, floor plan | draw.io |
| **Engineering** | electrical symbols, P&ID (process), fluid power | draw.io |
| **Mockups** | wireframe widgets, Android, iOS, Bootstrap, Google Material | draw.io |
| **Signs and symbols** | transportation, healthcare, safety, sports, nature… | draw.io |
| **Brands** | 3,466 brand logos in their brand colors | [Simple Icons](https://simpleicons.org) |
| **General icons** | Tabler (5,184), Lucide (1,891), Material Design Icons (7,447) | MIT / ISC / Apache-2.0 |

## Colors

Monochrome sets (diagram shapes, engineering, mockups, general icons, many generic sets) are stored in white and **tinted with the object's outline color**: pick another color in the style panel and the icon follows, in the application and in every export. Vendor sets (AWS, Azure, Google Cloud, Cisco…) and brand logos keep their own colors.

## Licenses and trademarks

- The draw.io libraries are under the **Apache License 2.0** (jgraph/drawio); Simple Icons is **CC0**; Tabler is **MIT**; Lucide is **ISC**; Material Design Icons is **Apache-2.0**. The license texts are in [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md).
- Logos and product icons (AWS, Azure, Google Cloud, Cisco, IBM, the brand logos…) are **trademarks or property of their respective owners**. They are provided, as in draw.io, to label diagrams of those products; Inkboard is not affiliated with or endorsed by them, and you remain responsible for complying with each owner's brand guidelines when you publish a diagram.
- Libraries that are proprietary to other tools (for example Miro's) are not included: they are not published under a license that allows redistribution.

## Keeping the library up to date

The library follows its upstream projects automatically. The workflow [`icons.yml`](../.github/workflows/icons.yml) runs **every Monday** (and on demand from the *Actions* tab): it fetches the latest draw.io, Simple Icons, Tabler, Lucide and Material Design Icons, rebuilds the pack, and compares it with the one in the repository.

- If nothing changed, it stops there.
- If icons were **added, removed or redrawn**, it checks the new pack (it parses, and every icon renders), refreshes the license texts and opens a **pull request** listing, set by set, which icons were added and removed and which upstream commits it comes from. Nothing is merged by itself: someone reads the summary and merges.
- If more than 25 % of the icons would disappear (an upstream restructuring, a broken fetch), it refuses and writes nothing: a person has to look.

Existing boards are never affected: an icon placed on a board is stored in the board file (the SVG itself), so removing it from the library cannot break a board, and updating its drawing upstream does not change boards that already contain the old one. New icons reach users with the next release of the application, since the library is part of the executable (the application never downloads anything). The palette's tooltip on the icon count shows which upstream commits the installed library was built from.

One-time setup for the repository: *Settings → Actions → General → Allow GitHub Actions to create and approve pull requests*. Pull requests opened by the default token do not start the CI by themselves; add a personal access token as the secret `ICONS_PR_TOKEN` to have it start (or close and reopen the pull request). To update by hand: `python scripts/build_icon_library.py --fetch ../iconsrc --report report.md`.

## Rebuilding the pack

The library is a single file, `crates/inkboard-app/assets/icons.bin` (about 8 MB, deflate-compressed, embedded in the executable), built by [`scripts/build_icon_library.py`](../scripts/build_icon_library.py) from shallow clones of the upstream projects:

```bash
mkdir iconsrc && cd iconsrc
git clone --depth 1 --filter=blob:none --sparse https://github.com/jgraph/drawio.git drawio
git -C drawio sparse-checkout set src/main/webapp/js src/main/webapp/img/lib
git clone --depth 1 --filter=blob:none --sparse https://github.com/simple-icons/simple-icons.git simple-icons
git -C simple-icons sparse-checkout set --no-cone icons data LICENSE.md
git clone --depth 1 --filter=blob:none --sparse https://github.com/tabler/tabler-icons.git tabler
git -C tabler sparse-checkout set --no-cone icons/outline LICENSE
git clone --depth 1 --filter=blob:none --sparse https://github.com/lucide-icons/lucide.git lucide
git -C lucide sparse-checkout set --no-cone icons LICENSE
git clone --depth 1 --filter=blob:none --sparse https://github.com/Templarian/MaterialDesign-SVG.git mdi
git -C mdi sparse-checkout set --no-cone svg LICENSE
python ../scripts/build_icon_library.py .
```

The pack in the repository was built from jgraph/drawio `17b367b` (2026-10-18), simple-icons `4d29a35` (2026-10-13), tabler-icons `a4ce140` (2026-10-05), lucide `1ef60af` (2026-10-16) and MaterialDesign-SVG `9e04201` (2023-12-26).

draw.io ships its shapes as *stencils* (vector drawing instructions) in a compact binary encoding inside `stencils.min.js`; the script decodes them and converts each one to SVG (paths, rectangles, ellipses, arcs, colors, dashes). `crates/inkboard-icons` reads the pack (index, search, SVG extraction), and `crates/inkboard-app/src/library.rs` is the palette. Run the full check of the pack with `cargo test --release -p inkboard-app -- --ignored every_library_icon_renders`. How the app stays fluid with thousands of icons on a board: [PERFORMANCE.md](PERFORMANCE.md).
