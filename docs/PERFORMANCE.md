# Performance with many icons

*Version française : [fr/PERFORMANCE.md](fr/PERFORMANCE.md)*

Icons are SVG images, which are expensive in three ways: rasterizing them, uploading the pixels to the graphics card, and drawing thousands of small textured quads. Inkboard keeps a board fluid with tens of thousands of icons by doing as little of each as possible.

## How it works

- **Resolution follows the screen.** An SVG is rasterized at the next power of two above the size it is shown at (64 to 4096 pixels, never above the graphics card's texture limit), so an enlarged or zoomed icon is sharp, and a small one is cheap. Several levels of the same icon can coexist.
- **Small icons share an atlas.** Icons shown at 30 px or less are packed into shared 2048 × 2048 texture pages: thousands of them are drawn in a single batch instead of one draw call each.
- **Tiny icons are flat quads.** Below 9 points on screen an icon is drawn as a flat square of its average color (no texture at all); the 3-pixel level of detail of every object still applies below that.
- **A time budget, not a freeze.** At most about 5 ms per frame is spent rasterizing; icons that do not fit wait for the next frame and show the closest texture already available (or a gray placeholder) meanwhile. A first view of a huge board fills in over a few frames; the interface never blocks.
- **Memory is bounded.** Textures that were not used recently are dropped beyond 256 MB; the atlas is shared.
- **Repeated icons are stored once.** Adding an icon that is already on the board reuses its asset (same bytes, same type): a diagram that uses a database icon 200 times stores, rasterizes and uploads it once, and the file stays small.
- **Culling.** Only the objects intersecting the view are visited (spatial index), as for every other object.

## Measuring it

```bash
inkboard --bench-icons 20000:3000      # 20,000 icons on the board, 3,000 of them different
```

It builds the board from the icon library and runs a scripted session — first views, steady view of everything, zooming in to 300 %, panning at 100 %, zooming out, enlarging one icon — while recording every frame: the CPU time of Inkboard's own frame code, and the **interval between frames** (which also includes drawing and the display refresh). It prints, for each phase, the median, 95th and 99th percentiles, the worst frame, the share of frames over 16.7 ms (60 fps) and over 33 ms, and how many SVGs were rasterized and for how long.

The unit tests check the mechanisms (power-of-two levels, texture size limit, shared atlas, bounded rasterization time per frame, closest-texture fallback, one rasterization per icon, repeated assets stored once); `cargo test --release -p inkboard-app -- --ignored every_library_icon_renders` renders every icon of the library.

## Results

Release build, Windows 11, Intel Core i9-12900K, NVIDIA GeForce RTX 5070, 1360 × 860 point window; the floor of about 5.5 ms is the frame interval of an idle repaint loop on this machine. Frame **interval** in ms (median / 95th percentile / worst):

| Icons (different) | Everything in view | Zoom in to 300 % | Pan at 100 % | Zoom out | Enlarge one icon |
|---|---|---|---|---|---|
| 1,000 (60) | 5.6 / 5.7 / 5.8 | 5.6 / 5.8 / 9.2 | 5.6 / 5.9 / 6.4 | 5.6 / 5.9 / 6.2 | 5.6 / 5.9 / 20 |
| 5,000 (2,000) | 5.6 / 5.8 / 6.0 | 5.6 / 8.6 / 10.7 | 5.6 / 5.8 / 10.7 | 5.6 / 7.4 / 8.3 | 5.6 / 5.8 / 14.7 |
| 20,000 (3,000) | 13.0 / 14.1 / 20 | 5.7 / 12.7 / 13.7 | 5.6 / 5.8 / 6.0 | 5.6 / 12.0 / 13.4 | 5.5 / 6.3 / 17.8 |
| 100,000 (5,000) | 19.5 / 25.2 / 80 | 5.7 / 15.6 / 20 | 5.6 / 5.8 / 6.0 | 5.6 / 13.5 / 17.5 | 28 / 31 / 40 |

In plain words: up to 5,000 icons nothing ever drops below 90 fps; with 20,000 icons all in view at once the board still runs above 60 fps and every zoom, pan and resize stays smooth; at 100,000 icons all in view it runs around 40–50 fps, and zooming or panning into any part of it is smooth again. The slow cell of the last row is resizing an icon on a 100,000-object board in the benchmark, where every step is a committed modification that rebuilds the 100,000-object index (18 ms); dragging a handle with the mouse has not been measured at that size.

Before this work, with 20,000 icons in view the same session ran at about 34 ms per frame (29 fps), and an enlarged icon was visibly blurry.
