# Preparing a public pull request

1. Update the session log, setup knowledge, architecture decisions and branch-status handoff.
   Keep local agent wrappers and machine-specific memory outside the commit.
2. Inspect every staged and newly added file, including fixtures and documentation. Search for
   credentials, private keys, personal paths, device identifiers, actual document names,
   private meeting/chat links and notification content. Review any scan finding manually;
   passing a pattern scan is not proof that every possible secret is absent.
3. Include generic application recipes and controller/adapter code. Keep API keys, pairing
   data, recorder/document bindings, notification captures, recordings, screenshots, flash
   backups and diagnostic exports in private local storage. Fixtures use synthetic data.
4. Check runtime data boundaries too: local path controls are editable locally; the setup
   assistant receives configured/existing flags, not paths. Notification bodies and slide
   notes stay out of logs and assistant diagnostics. Production-side notification tests and
   hardware confirmation are recorded separately from unit tests.
5. Run checks appropriate to the change and inspect their output. Record OS, board model,
   firmware/capabilities and limitations without publishing unique device/recovery identifiers.
6. Verify the repository, authenticated GitHub identity and write permission before pushing.
   Open a draft PR for review; shared protocol/schema changes need both maintainers before
   merge. Link the relevant implementation decision and state unverified behaviour clearly.

Privacy redactions do not remove data from earlier published Git history. If a real credential
is discovered, revoke it through its provider; deleting a current-file line is insufficient.

If personal login succeeds but Git still cannot obtain credentials, inspect host-specific helper
configuration for an obsolete portable CLI. Override the helper and username only in this
repository's local Git configuration, then verify the authenticated GitHub identity and repository
write permission. Keep credential values out of command output, source and reports; do not change
other projects' authentication to repair this one.
