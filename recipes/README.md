# Recipes

Keys for apps, websites and coding agents, as data-only TOML layout libraries
([`spec/protocol.md`](../spec/protocol.md) section 5, schema
[`spec/schemas/library.schema.json`](../spec/schemas/library.schema.json)). Every file here is compiled
into `knoend` and checked by `python3 tools/validate_spec.py`.

```
apps/      desktop apps: one file per app for every OS (match.linux / macos / windows, Primary shortcuts)
sites/     websites: <host>.toml for the whole site, <host>/<page>.toml for one kind of page
agents/    coding agents' screens on the board (claude-code/permission, /question, /elsewhere)
```

## Working recipes

A recipe says where it was seen working in `[[pack.tested]]` (OS, who, date, desktop or browser).
An OS without an entry is untested there. To add your test, append an entry; to make an app recipe
work on another OS, add its `match` entry for that OS (Linux: WM_CLASS or desktop id; macOS: bundle
id; Windows: executable), check the shortcuts there, then add your `tested` entry.

## From an AI draft to a recipe

"Configure" on the board drafts keys into `~/.config/knoend/libraries/ai/` (Linux) on the computer
that asked. Once you have used a draft and it works:

1. Copy it here (`apps/` or `sites/`), set `source = "builtin"` and an id starting `builtin.`.
2. Take out anything personal or local: your own server's address, account names, private URLs.
   A recipe should work for anyone (recipes for this project's own GitHub pages are the exception).
3. Add a `[[pack.tested]]` entry with your name, the OS and the date.
4. Run `python3 tools/validate_spec.py` and the companion's tests, then open a pull request.

A recipe here takes precedence over an AI draft of the same page; your own libraries in
`~/.config/knoend/libraries/` (source `user`) take precedence over both.
