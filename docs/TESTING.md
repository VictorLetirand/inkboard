# Testing Inkboard in 10 minutes

*Version française : [fr/TESTING.md](fr/TESTING.md)*

Thank you for trying Inkboard! The current versions are **release candidates** (`v0.2.0-rc.N`): they have been used by hand on Windows only.
Linux and macOS builds pass the automated tests but nobody has used them by hand yet, and the pen has not been validated on real hardware.
Every report helps, whether it says "all good" or "this broke".

## 1. Get it

Download the installer for your system from the [**Releases**](https://github.com/VictorLetirand/inkboard/releases) page (the newest `rc`):

| System | File |
|---|---|
| Windows 10/11 | `inkboard_*_x64-setup.exe` |
| Linux (x86-64) | `inkboard_*_amd64.deb` or `inkboard_*_x86_64.AppImage` |
| macOS | `Inkboard_*.dmg` |

The installers are **not code-signed yet**, so your system warns you the first time: on **Windows**, SmartScreen says "Windows protected your PC" → *More info* → *Run anyway*;
on **macOS**, Control-click the app → *Open*. Every release lists SHA-256 checksums. No install at all? Join a shared board from a web page (see step 8).

## 2. Things to try

1. **Pen:** draw a few freehand strokes. In the style panel of the pen, give one stroke an **arrowhead at the end**.
2. **Shapes and text:** draw a rectangle, an ellipse and a diamond. The text opens right after the shape is drawn: type in it.
3. **Connectors:** join two shapes with an arrow, then move a shape: the connector must follow.
4. **Table:** insert a table, type in a few cells, add a row.
5. **Paste from a spreadsheet:** copy a few cells in Excel / LibreOffice / Google Sheets and paste them on the board.
6. **Export:** export the board as **PNG** and as **PDF**, and open the results.
7. **Open a foreign file:** open a draw.io or Excalidraw file you already have (`.drawio`, `.excalidraw`).
8. **Live sharing:** click *Share* and join from a second device or a browser (the invitation link opens in a web page, nothing to install). Do both sides see each other's changes and cursors?
9. **Dark theme:** switch to the dark theme from the menu and look around.

## 3. If you have the right system or device

- **Linux:** try the AppImage (`chmod +x *.AppImage && ./*.AppImage`) and the `.deb` (`sudo apt install ./inkboard_*.deb`). Tell us your distribution and whether you use X11 or Wayland.
- **macOS:** try the `.dmg` (Apple Silicon or Intel) and tell us which.
- **Surface / pen users:** run `inkboard --pen-test` and check pressure and the eraser end, then send the report ([how](STYLET.md)).

## 4. Tell us how it went

- Fill in the [**Test report**](https://github.com/VictorLetirand/inkboard/issues/new?template=test_report.md) form: version, system, what worked, what did not (screenshots welcome).
- Questions, ideas, or just "it works great": [GitHub Discussions](https://github.com/VictorLetirand/inkboard/discussions).
- If Inkboard crashes, it writes a log and tells you where: `%APPDATA%\Inkboard\crash.log` on Windows. Please attach it.
