# Native Windows adapters and configurable presentation modes

Status: local prototype, shared schema changes require both maintainers' review before merge.

Presentation labels, slots, modes, shortcuts, entry actions, notes policy, pointer gain and
capability fallback belong in the recipe. A shared controller validates and renders them;
platform adapters retain app detection, COM/UIA access and pointer targeting. This avoids
rebuilding for layout changes without exposing arbitrary process execution to recipes.

Windows PowerPoint and pager workers use the same Rust executable as supervised child
processes. Direct COM/UIA replaces the PowerShell scripts while preserving timeout isolation.
A native local launcher configures runtime and editable recipe paths. No firmware change is
needed, and the user's independently configured recorder launcher remains supported.

PPTX ZIP/XML notes extraction is retained. Windows uses an exact explicit binding or the
native reader's exact active path, never recent-file guesses. Live notes can include unsaved
edits; saved extraction follows relationships and only contains saved content. The dashboard
and setup assistant expose source selection, binding and diagnostics.

Rejected: hardcoded Rust key maps; per-recipe shell scripts; most-recent-file notes fallback;
running potentially blocked COM/UIA providers inside the Bluetooth service loop. Google Slides
still needs a browser adapter; a PowerPoint provider is not silently applied to browser tabs.
