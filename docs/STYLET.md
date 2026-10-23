# Validating the pen on real hardware

*Version française : [fr/STYLET.md](fr/STYLET.md)*

Pen pressure and the pen eraser are **implemented and automatically tested, but have never been tried with a real pen** (no digitizer was
available during development). The diagnostic mode lets you check it in two minutes.

## Procedure (Windows)

```bash
inkboard --pen-test
```

A "Pen diagnostics" panel appears on the right. On a pen-enabled device (Surface, Wacom tablet, pen touchscreen…):

1. **Draw** with the Pen tool, varying the pressure: the stroke should get thicker as you press. In the panel, "Pen detected by Windows",
   "Variable pressure" and "Pressure received by the application (egui)" should turn **yes**.
2. **Flip the pen** (eraser end) and pass over a drawing: it should be erased, whatever the active tool. "Eraser end detected" → yes.
3. Press the **barrel button** ("Barrel button detected") and tilt the pen ("Tilt detected"): informational.
4. Click **Copy the report** and paste it into a message: it contains the verdicts and the measurements (number of pressure levels, etc.).

## Reading the results

| Observation | Meaning |
|---|---|
| "Pressure received by Windows" yes but "by the application (egui)" NO | Windows sees the pressure but egui does not pass it on: variable width will not work; the fix is to read the pressure directly from `inkboard-pen` (already available in `PenSample`). |
| "Variable pressure" NO with few levels | The driver returns a binary pressure (0/1) or the mouse is emulating the pen: check the driver / Windows Ink. |
| "Eraser end" NO although the eraser end is used | The driver does not set `PEN_FLAG_INVERTED` / `PEN_FLAG_ERASER`; some pens have no hardware eraser. |
| Last pointer "mouse" while using the pen | Windows Ink is disabled for the application or the driver emulates the mouse. |

## What is verified without hardware

- The interpretation of the pen flags (`inkboard-pen`) and the construction of the verdicts (`inkboard-app/src/pen_test.rs`).
- The installation of the window *subclass* on a dummy window that does not alter the messages.
- The application's behavior when the eraser is detected (the gesture erases whatever the active tool) and the rendering of width according to pressure.
