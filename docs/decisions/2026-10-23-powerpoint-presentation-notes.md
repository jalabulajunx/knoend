# Pure Rust in-memory PowerPoint presentation notes and pagination

## Context

PowerPoint presentation support was requested (#78) to display active slide speaker notes on the
board and provide navigation controls (Prev, Next, First, End, Black screen, White screen).

Initial prototypes attempted using Windows COM automation through PowerShell, and falling back to
the most recent `.pptx` in `Recent` / `Downloads` when window titles lacked full file paths.
Furthermore, slide speaker notes longer than 240 bytes were truncated or rejected by firmware.

## Decision

1. **Pure Rust in-memory extraction:**
   - Parse `.pptx` (ZIP archive containing `ppt/slides/slide*.xml`, `ppt/notesSlides/notesSlide*.xml`
     and relationships) directly in Rust using the `zip` crate.
   - Synchronous, instant (<2 ms), zero lag, cross-platform, and immune to Office Protected View,
     modal dialogs or licensing prompts.
   - In-memory notes are destroyed (`powerpoint::clear()`) as soon as PowerPoint closes, loses focus,
     or presentation ends.

2. **Dedicated `system.presentation` layout with note pagination:**
   - Modeled after Claude approval screens (`system.approve`): top row has Prev (1), End (2), Next (3);
     bottom row has First (7), Note page cycler / Black screen (8), White screen (9); middle rows
     (4, 5, 6) are left empty so firmware renders the detail panel cleanly.
   - Firmware enforces `UI_DETAIL_BYTES 241` per detail packet. Rather than truncating long notes,
     `paginate_notes` splits text on line/word boundaries into pages of $\le 220$ characters with
     smart punctuation normalization (`'`/`"`, `-`, `*`).
   - For multi-page notes, Key 8 becomes a page cycler (`1/3`) and touch swipes (`Event::Page`) flip
     pages without advancing slides.

3. **Strict mode separation & no heuristic fallbacks:**
   - Normal PowerPoint edit mode displays standard editing keys (`recipes/apps/powerpnt.toml`: Open,
     New Slide, Save, Undo, Present, Redo, Duplicate, From Current, Laser/Pen).
   - Presentation mode activates only when window title indicates Slide Show or user taps Present/F5.
   - If an unsaved presentation or template has no saved file on disk, the companion initializes an
     in-memory session with `"No notes for this slide"` rather than guessing a past presentation from
     `Recent` or `Downloads`.

## Alternatives rejected

- **COM / PowerShell Automation:** Spawns external processes, introduces 200–500 ms lag, fails on
  Protected View or sandboxes, and Windows-only.
- **Recent / Downloaded file heuristic fallback:** Disastrous when creating new presentations or
  templates, as it pollutes new documents with stale notes from completely different presentations.
- **Hard text truncation:** Truncating at 240 characters cuts off sentences and bullet points midway.
