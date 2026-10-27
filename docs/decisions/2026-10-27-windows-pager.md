# Windows desktop pager and local recorder action

Add Windows desktop recipes for ChatGPT/Codex and Teams. Keep shortcuts as validated recipe
data and use the existing named-command action for `start-steno`. The command reads one local
`.vbs` launcher path from configuration; recipes cannot supply executable paths or arguments.
Opening that launcher does not imply starting a recording.

Capture visible Windows toasts and incoming Teams call windows in an isolated, read-only
native Rust UI Automation worker. Queue bounded transient alerts in the companion, ahead of
normal app keys but below agent approval requests. Keep notification bodies in memory; send
them only to paired boards and never to the setup assistant or service log. The assistant
gets settings, capture diagnostics and a synthetic-test action.

The authenticated loopback pager endpoint also accepts validated, bounded notices for the
three supported app names, enabling deterministic tests and a future native notification
source without changing the board protocol. Each displayed alert carries a revision in its
layout id, so old taps cannot act on a newer alert. Dismissal and expiry restore app keys.

Alternatives: Windows UserNotificationListener needs a package capability and user consent
flow; the current portable companion has neither. Reading notification databases or Teams
history would broaden access unnecessarily. UI Automation therefore captures only visible
notifications, with explicit gaps for suppressed toasts and unrecognised locale/version
labels. No claim of complete notification coverage is made. See `../windows-pager.md`.

The initial PowerShell prototype was replaced by the supervised same-executable adapter;
see [the native-worker decision](2026-10-27-native-recipe-presentations.md).
