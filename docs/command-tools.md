# Command tools

`CommandTools` classifies execution as a general command, test, formatter, linter, type checker, or
build. Each category has an operator-configured list of allowed argv prefixes. A request must match
one non-empty prefix in its declared category before it can reach the sandbox.

Successful dispatch returns the category, exact argv vector, exit status, timeout state, duration,
and output artifact references. The command layer does not weaken sandbox path, environment,
network, or resource policy. Prefixes should include fixed subcommands (for example `cargo test`)
rather than broad executable-only entries when narrower authorization is possible.
