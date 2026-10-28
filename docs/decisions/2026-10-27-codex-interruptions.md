# Native Codex lifecycle bridge

Local prototype on sundar/codex-interruptions; based on the Windows integration in draft PR #90.

Reuse the authenticated/private agent bridge and shared approval renderer, with provider-specific
recipes and response encoding. Codex supports one-request allow/deny; never copy Claude's
persistent permission, updated-input or stop-agent output into Codex. Bound stdin and omit
unneeded prompt/transcript/cwd/answer fields. Fully display approval inputs or hide Allow.
Keep decisions explicit, provider/session/turn-scoped, and stale-layout protected.

Use a separate recipe for completion alerts through the bounded pager. Deduplicate session/turn
completion reports and retain approval priority. Generic completion text avoids response leakage.

Prepare user-level native executable hooks additively with a first backup and atomic write;
preserve existing hooks and notify configuration. Supply the running service's endpoint file
explicitly for custom runtime homes. Codex's exact-definition trust review remains the user's
step. Do not modify trust storage, approval policy, managed policy or launch with bypass flags.

Rejected: scraping live transcripts for questions/approvals, UI keystroke approvals, replacing the
existing notify callback, assuming an installed file is trusted, and claiming a synthetic test is
producer-side delivery. Cloud orchestration and ordinary question answers need a separate,
supported contract before they can be promised.
