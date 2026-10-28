# PowerPoint controls on Windows

KnoEnd switches from editing keys to presentation controls when a PowerPoint show starts, including Presenter View and shows started from the PC keyboard. Ending the show restores editing keys. Switching to another app selects that app's controls.

## Presentation keys

| | | |
|---|---|---|
| Previous | Black screen | Next |
| Laser | Notes | End show |
| First slide | White screen | Arrow |

Black and White toggle the corresponding blank screen. Arrow restores PowerPoint's ordinary arrow pointer.

## Laser touchpad

On touchpad-capable firmware, Laser centres the pointer in the show window and opens a drag surface. Drag your finger to move the laser; lift and reposition your finger to start another stroke without jumping the PC pointer. The bottom row has Previous, Controls and Next. Controls restores the arrow and returns to presentation keys.

The surface moves the pointer only. Taps do not click, draw ink or advance slides; use the explicit slide buttons. The pointer stays within the slide-show monitor, including a second monitor. Initial movement gain is two desktop coordinate units per device pixel. Older firmware retains the eight-direction button pad.

Normal operation works over the Bluetooth data connection. Keep the board powered and the companion running on the computer. USB is required for the firmware backup and installation, rather than ordinary wireless use.

## Speaker notes

Notes opens the current slide's speaker notes, with Previous/Next slide, Previous/More notes pages and Controls to return. On Windows, a read-only worker attaches to the already-running PowerPoint application and follows the actual displayed slide, including navigation from the PC and animations. It reads the live note body, including unsaved text, without opening, editing or saving presentations.

Notes are split into word-boundary pages that fit the display. Empty notes explicitly say No notes for this slide. If the live reader is unavailable, the panel reports that separately. Notes and file names remain in memory; diagnostics contain reader freshness, availability, slide index/count and note presence, not note text. The setup assistant's powerpoint_context check also reports the board's touchpad capability.

The native Rust worker uses Windows COM directly, in a supervised child of the companion executable. It does not launch PowerShell. A hung COM provider can be restarted independently of Bluetooth and the web app.

The original ZIP/XML extractor remains in `powerpoint.rs`. On Windows the recipe chooses `live`, `pptx` or `live_or_pptx`. The dashboard's Presentation settings can override this policy and bind an exact saved `.pptx`. With no binding, extraction uses the native reader's exact active FullName. A conflicting binding is rejected at display time; recent files are never guessed. Saved extraction cannot see unsaved changes. Slides follow the presentation's relationship order, and notes follow each slide's notes relationship; XML entities are decoded. Files are read in memory, with no unzip folder. File metadata changes invalidate the cache.

Legacy non-Windows notes resolution uses saved-PPTX resolution and estimated navigation; it does not have the live Windows reader.

## Configure presentation recipes

`recipes/apps/powerpnt.toml` owns presentation mode names, labels, slots, grids, shortcuts, mode-entry shortcuts, pointer gain, notes policy and touchpad fallback. The Rust controller executes validated actions and renders the selected mode; PowerPoint detection, COM and pointer targeting remain native adapter capabilities.

For example, change `presentation.pointer_gain` to adjust movement speed, or edit a `[[presentation.modes.controls.key]]` label and slot. A mode key uses `action = { kind = "presentation", command = "mode", mode = "pointer" }`. Modes with `touchpad = true` reserve the bottom row for buttons; `notes = true` reserves the middle rows for text. `fallback` names a mode for boards without a touchpad or sufficient grid space.

Use Reload recipes in the dashboard after saving. Reload validates the complete catalog before swapping it; an invalid mode, target, fallback cycle, shortcut or placement leaves the current catalog running. The local launcher points at the source recipe directory, so edits do not require rebuilding or reflashing. Optional user recipes in the local configuration's `libraries` folder retain the normal catalog precedence.

The Google Slides recipe still uses its existing browser shortcuts. This change establishes recipe-driven presentation modes for PowerPoint; Slides context, pointer activation and notes need a browser adapter before the same modes can be promised there.

## Installation and verification

The local 0.2.3-touchpad1 firmware adds the drag surface and relative pointer reports. Two reads of the current partitioned flash region must match before installation. Check the saved partition table, preserve NVS, bootloader, board definition and fonts, and write only the application partition. Verify the image and protected regions before normal boot. Reading or writing flash temporarily puts the board in its loader and blanks the screen; reset it back to the installed application afterward, including cleanup after failed reads.

ONX2432G028 validation: user confirmed Notes text on the device, finger dragging after installation, and finger dragging over Bluetooth after removing PC USB data. The companion exposes firmware/capability and link health for troubleshooting.

See [the implementation decision](decisions/2026-10-27-touchpad-live-powerpoint.md) for protocol details and [Microsoft's PowerPoint presentation shortcuts](https://support.microsoft.com/en-us/accessibility/powerpoint/use-keyboard-shortcuts-to-deliver-powerpoint-presentations) for the underlying commands. These protocol additions remain a local prototype requiring both maintainers' review before merge.
