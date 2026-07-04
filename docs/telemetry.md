# Telemetry And Benchmarks

Interactive turns write JSONL telemetry to `.mc/telemetry.jsonl`. Benchmark runs write JSONL telemetry to `.mc/benchmark.jsonl`.

Each record includes:

- mode and edit strategy
- task id
- estimated prompt, completion, and total tokens
- wall time
- command count
- edit bytes
- repair turns
- test result

Run benchmarks with:

```sh
cargo run -- bench
```

Benchmark fixtures live in `fixtures/benchmarks/*.json`. The report compares the configured strategies for each task and prints token totals, command counts, edit bytes, elapsed time, and verification status.
