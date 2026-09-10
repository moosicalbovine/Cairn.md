# Cairn.md performance contract

Cairn.md v0.1.0 targets a responsive Windows desktop experience without bundling a browser runtime. Release candidates are measured from an x64 release build on a documented reference Windows profile.

## Release thresholds

| Metric | Required result | Current evidence |
|---|---:|---|
| Cold start to an interactive window | p95 at or below 1.5 seconds | Harness implemented; reference run pending |
| Editor input to painted frame | p95 at or below 50 ms | Harness pending |
| Normal document open | p95 at or below 250 ms | Harness pending |
| Idle process-tree working set | Below 150 MB after 60 seconds | Harness implemented; reference run pending |
| Durable recovery lag | At most 2 seconds | Deterministic autosave tests implemented; process-termination harness pending |
| Stress integrity | No crash or unintended byte change | 1,000-file reconciliation and 10,000-edit tests implemented |

No result is marked as passing until raw samples from the reference profile are retained. Hosted CI results may be useful comparisons, but they do not replace the release profile because virtual-machine load and WebView2 versions vary.

## Startup and memory measurement

Build the release binary, then run at least 20 independent launches:

```powershell
npm run tauri build
npm run perf:startup -- "src-tauri\target\release\cairn-md.exe" --samples 20 --idle-seconds 60 --output "performance-results\startup.json"
```

The harness measures elapsed time from process creation until Windows reports Cairn.md's main window, closes the full process tree between samples, and separately records the process-tree working set after the idle interval. A non-zero exit means at least one enforced threshold failed.

This is currently a conservative process/window baseline, not yet the complete V7 startup proof: the final harness must also observe that the library and editor accept input. Input-paint and document-open measurements likewise remain required before release sign-off.

## Stress coverage

- `src-tauri/tests/reconcile_stress.rs` creates 1,000 Markdown files, applies external edits and renames, reconciles repeatedly, restarts the service, and verifies stable identities and exact content.
- `tests/e2e/stress.spec.ts` applies 10,000 sequential source edits and 1,000 cross-region visual edits while repeatedly switching editor modes.
- Recovery tests exercise every save-journal phase, external conflicts, missing files and projects, and two consecutive restarts.

## Required result record

For each release candidate, retain the JSON output and add the following to this document:

- Cairn.md commit and version
- Windows build and WebView2 version
- CPU, installed RAM, and storage type
- Raw sample artifact location
- p50, p95, and maximum for each latency metric
- Idle process-tree working set
- Pass or fail against every threshold
- Any discarded harness run and its documented reason
