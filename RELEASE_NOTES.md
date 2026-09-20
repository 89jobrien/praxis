# Unreleased

## What's New

Praxis can now ingest replayable Crux traces from explicit local files or directories:

```bash
praxis ingest --strategy .praxis/strategy-history.json traces/
```

Directory scans are recursive, ignore symbolic links, and reject presentation-only or malformed traces with clear diagnostics. Successfully processed trace IDs are recorded beside the strategy history, so rerunning the command skips completed work while failed traces remain available for retry.

Praxis also supports concurrent batch evaluation and approval-gated strategy improvements, making it possible to evaluate multiple agent runs while retaining control over higher-risk changes.

## Improvements

- Help output now documents both demo and trace-ingestion modes.
- The ingestion summary reports scanned, ignored, discovered, processed, duplicate, rejected, and failed trace counts.
- Strategy and ingestion state are validated when opened instead of silently accepting malformed files.

## Compatibility

Existing demo commands continue to work unchanged. Local ingestion requires raw replayable JSON produced by `crux run --save-trace`; presentation JSON is intentionally rejected.
