# Touch surface and live Windows PowerPoint state

Date: 2026-10-27. Local prototype; protocol additions require both maintainers' review before merge.

## Problem

The direction-button laser pad works over Bluetooth but requires repeated taps. Notes inferred from window titles and files in a few folders cannot reliably identify bracketed filenames, presentations elsewhere, unsaved text, or the current slide after animations and PC navigation.

## Decision

Firmware that supports a drag surface advertises features.touchpad=true. An optional layout.touchpad=true replaces the rows above the bottom controls with a relative pointer surface. It sends knoend.pointer reports containing layout, bounded integer dx/dy and a per-boot sequence. Reports are limited to approximately 30 Hz; finger-down anchors the stroke, finger-up ends it, and no click or slide-advance event is generated. Older firmware keeps the direction pad.

The companion rejects pointer input outside its current presentation touchpad panel, during editing/approval/pager overlays, or without a foreground PowerPoint show. It deduplicates reports across data transports and confines the PC pointer to the slide-show monitor. Initial gain is two desktop coordinate units per device pixel. The bottom row retains Previous, Controls and Next.

On Windows, a separate native Rust COM worker attaches to the existing PowerPoint instance through the running-object table. It reads the actual slide-show slide and notes body; it never opens files, changes slides, edits notes or saves the presentation. Notes and file names stay in worker stdout and companion memory; diagnostics expose only reader health, slide counts and note presence. A stale worker is restarted after 30 seconds. The service renders note pages within the firmware's existing 220-byte budget. Reader failures are distinct from a slide with no notes.

## Alternatives

Companion-only direction buttons cannot produce drag coordinates because existing firmware does not transmit finger motion. Filename search remains the legacy non-Windows fallback, rather than the Windows authority. No recent-file guessing, recording integration change, external publication or data upload is introduced.

## Installation and verification

Build from a disposable local source copy. Back up and compare the current partitioned flash region before writing, check its partition table, and write only the application partition. Preserve the board definition, NVS and fonts. Verify the compiled image and protected regions before boot. One firmware feature change per flash, then user validation on the ONX over Bluetooth. A successful compile is not physical touchpad or notes validation.

The initial PowerShell reader and fixed presentation key maps were replaced by native
workers and recipe modes; see [the follow-up decision](2026-10-27-native-recipe-presentations.md).
