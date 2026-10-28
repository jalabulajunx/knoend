# Codex interruptions

Local Codex desktop and terminal sessions can use the same native KnoEnd bridge for approval
requests and turn completion. The companion and board need their existing data connection;
no firmware change is required. Cloud-orchestrated sessions and ordinary question-card replies
are outside this hook integration.

## Prepare and enable

In the dashboard, open Codex interruptions and choose Prepare hooks. This adds native executable
commands to the user-level Codex hooks.json, preserves other hooks, backs up the original once,
and leaves config.toml and its existing notify callback unchanged. Advanced CLI setup is
`knoend codex-hooks install` from the same runtime environment as the running companion.
The generated commands include the private endpoint file so custom runtime folders work.
Windows native hook paths currently need no spaces or shell metacharacters; choose a local
runtime accordingly. No PowerShell script is generated.

Review and trust these exact hook definitions using Codex's /hooks UI, then use a new local
session or restart the relevant session. File presence alone does not prove trust, host support
or delivery. Codex may restrict hooks through managed policy. KnoEnd never changes approval
policy or bypasses hook trust. Full-access/never-ask sessions may have no approval events.

The Codex interruptions setting disables the bridge's display/decision handling. Completion
alerts also use the pager enable setting. Remove hooks removes only KnoEnd's Codex handlers,
including when grouped with another handler. Other hooks and the notify callback remain intact.

## Behaviour and recipes

`recipes/agents/codex-permission.toml` defines the permission screen: Allow once and Deny.
Only a deliberate tap returns a decision. Timeout, missing service or disconnected request
returns no decision, leaving Codex's normal approval flow in charge. No Always, Stop-agent,
updated-input or persistent-permission response is emitted. Full tool input must fit the
board's readable detail budget; otherwise Allow is hidden and the person reviews it in Codex.
Request-specific layout IDs reject stale taps. Provider/session/turn checks prevent unrelated
Claude or Codex events from clearing a request.

`recipes/agents/codex-completion.toml` defines Open Codex and Dismiss. A Stop event means a turn
ended, not that the task succeeded. Alerts remain for five minutes or until dismissed; a new prompt or interrupt clears that session's completion alerts. Repeated reports for the same session/turn are deduplicated.
Terminal hosts identified through tty or terminal environment metadata are brought forward;
other hosts use the Codex desktop window. Window selection does not identify an exact terminal
tab/chat. Ordinary Codex questions remain in its UI; no transcript scraping is used.

After editing recipes, use Reload recipes. Permission requests keep priority over completion
and pager alerts. Hook callbacks discard prompt, response text, cwd and transcript paths;
only approval tool input travels to the private local bridge and paired board. It is not logged
or put in assistant diagnostics. Completion text is generic. KnoEnd's own helper Codex calls
explicitly disable hooks so setup-assistant turns do not create completion interruptions.

## Verify

The codex_agent assistant check reports installation and content-free event counts; trust is
explicitly unknown. Test completion screen is synthetic and does not establish Codex delivery.
Validate a real producer-side Stop event from both a new trusted terminal session and a new
local desktop session. Actual approval requests also require a policy/session that would ask
for approval. A synthetic approval test exercises the board/bridge response but executes no tool.

Official contract: https://learn.chatgpt.com/docs/hooks.
