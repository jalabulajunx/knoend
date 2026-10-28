# Windows keys and pager

The ChatGPT / Codex and Microsoft Teams desktop recipes each have two 3x3 pages. Swipe to
change pages. Keys follow the focused desktop app. These recipes do not apply to a browser
tab or Codex CLI. The first ChatGPT page is shared navigation; page two includes Codex-only
attention, terminal, panel, project, review and file controls. Shortcuts can depend on the
focused panel and any custom bindings in the app.

Teams page one has microphone and camera toggles, raise/lower hand, the sharing tray, Start
Steno, leave call, audio answer, decline call and shortcut help. Leaving, answering and
declining require a second tap. These are keyboard shortcuts, not live mic/camera state.
Page two has search, go to chat, unread chats, chat list, messages, compose focus, back,
next section and settings. Compose focuses a field; it does not send a message.

## Pager interruptions

On Windows the companion starts an isolated, read-only native Rust UI Automation watcher.
It follows visible notification toasts attributed to Teams, ChatGPT or Codex and Teams
incoming-call windows with both answer and decline controls. It does not read chat history,
the Windows notification database, Microsoft Graph, or all ordinary window contents.
Notifications suppressed by Windows, Do Not Disturb or the app are unavailable. Incoming-call
control matching currently recognises the English and French labels in the native adapter;
different versions/locales may need additional labels. Failure to capture a real call must
be reported as a capture gap, not as proof that no call arrived.

An alert temporarily replaces app keys. Open app brings its window forward; Dismiss restores
app keys. Teams alerts also offer Start Steno. Prev text and Next text page the detail into
ASCII word-bounded chunks below the firmware's 241-byte buffer. Calls take priority over
messages. At most eight alerts remain in memory; calls expire after 90 seconds and messages
after 45 seconds from receipt. A disappearing incoming-call window withdraws its matching
alert. Agent permission requests have priority over pager interruptions. Editing keys pauses
the interruption display. Notification text is not written to the service log or included in
the setup assistant's diagnostics. Text is sent locally to paired boards for display.

On the dashboard, use Windows pager to enable interruptions and select an existing absolute
`.vbs` recorder launcher. The local path is saved only in `pager.json` under the companion's
configuration folder, never in a recipe. Start Steno opens that launcher; recording still
starts through the recorder's own existing controls. Recipes can name `start-steno` but cannot
provide a command line or launcher path. No automatic recording, answering or message sending.

Test pager queues a synthetic incoming call without making a call or starting a recording.
`GET /api/pager` reports capture and recorder availability without notification content.
`POST /api/pager` accepts bounded notices for teams/chatgpt/codex only; `POST /api/pager/test`
queues the synthetic alert. These routes require the existing local session token and Host/
Origin checks. Disabled pager interruptions discard notices. The setup assistant sees settings,
a `windows_pager` check, and buttons to open pager settings or run its synthetic test.

## Run a local build

Build the companion with Cargo's target directory and dependency caches outside OneDrive.
Run `knoend local --runtime <absolute-local-runtime> --recipes <absolute-recipe-folder>` to use
the checkout's editable recipe tree. This native launcher keeps pairing/config in
`config/knoend`, temporary files in `temp` and service logs in `logs` under the local runtime.
Choose a runtime outside OneDrive. Use the dashboard's Reload recipes after a recipe edit.
This launcher does not install a login task or change the recorder's startup configuration.

The native adapter runs as `knoend __native-worker pager`; `--once` performs one read-only
capture pass. It is normally supervised by the companion rather than started manually.
Rust tests check attribution, observed Teams banner classification, incoming-call controls,
notice validation, detail paging, queue precedence,
deduplication, bounds and the Windows recipe names. Physical key presses and real Teams
notification/call delivery require separate live validation.

Shortcut references (checked 2026-10-27):

- [ChatGPT / Codex desktop commands](https://learn.chatgpt.com/docs/app/commands)
- [Microsoft Teams keyboard shortcuts](https://support.microsoft.com/en-us/accessibility/teams/keyboard-shortcuts-for-microsoft-teams)
- [Microsoft UI Automation](https://learn.microsoft.com/en-us/windows/win32/winauto/ui-automation-specification)

Custom key bindings in either app take precedence over these defaults.

## Teams banners and capture health

The detector examines separately identified Teams-owned popups and Windows-hosted toast windows;
normal Teams chat and meeting windows are excluded before accessibility-tree traversal. Application
attribution can come from the popup root or a non-Text control, not only a Text element. A notification
centre/history window is excluded, and an explicit attribution to another application takes priority
over a mention of Teams in the notification body.

The dashboard's capture status now requires a completed scan in the last 15 seconds. The
windows_pager check includes last_scan_age_ms and content-free scan counts (roots, candidates,
matched and errors). A worker stalled for 45 seconds is terminated and restarted; starting the
process alone does not mark capture ready. These diagnostics do not retain notification text.

For a producer-side test, Teams offers Test notification in Settings > Notifications and activity.
This is different from KnoEnd's synthetic Test pager: it tests Teams-to-pager capture as well as the
board display. See Microsoft's notification settings guidance:
https://support.microsoft.com/en-us/teams/notifications-settings/manage-notifications-in-microsoft-teams
A passing synthetic test, scan heartbeat or recognition fixture does not prove that a real Teams
banner was captured; validate the Teams-produced test banner or a real incoming message separately.

The Teams-generated test exposed an unnamed native TeamsWebView popup (372x136 in the observed Windows session) with extended style 0x00200088: TOOLWINDOW and TOPMOST. Its native title did not contain notification or test. Match that compact Teams-owned tool-window shape without requiring a caption, while excluding ordinary Teams windows. The worker enumerates visible native windows for supported processes, then uses UI Automation only to read a selected popup. This avoids depending on tool windows appearing among UI Automation desktop children. The structural fixture uses the observed class, dimensions and style flags; live delivery on the board must still be verified separately.

For a positively identified native Teams banner, interruption is emitted before accessibility text
is read. If that provider is unavailable, the board still gets a generic Teams notification with
Open Teams and Start Steno. A successful later text read enriches the same alert. Visibility
generations distinguish a banner that hides and reappears, including repeated identical tests;
repeated scans while it stays visible are deduplicated.
