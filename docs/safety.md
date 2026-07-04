# Command Safety

The harness has two command paths:

- Safe context commands are structured slash commands: `/rg`, `/files`, `/head`, `/tail`, `/wc`, `/sed`, `/awk`, and `/context`.
- Shell escape commands use `!<command>` and are disabled unless `MC_ENABLE_SHELL=1` is set.

Safe context commands enforce workspace-relative paths, reject `..`, skip `.git` and `target` where directory walking is involved, and cap recorded output before adding it to the context ledger.

Use shell escape only for commands that cannot be expressed as structured context commands. Shell output is recorded as an activity action on the active conversation turn, but secrets and mutating commands are the user's responsibility when shell execution is enabled.
