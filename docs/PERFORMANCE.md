# Cairn.md performance contract

Cairn.md v0.1.0 targets a responsive Windows desktop experience without bundling a browser runtime. Release candidates are measured from an x64 release build on a documented reference Windows profile.

## Release thresholds

| Metric | Required result | Current evidence |
|---|---:|---|
| Cold start to an interactive editor | p95 at or below 1.5 seconds | Harness implemented; reference run pending |
| Editor input to painted frame | p95 at or below 50 ms | Harness implemented; reference run pending |
| Normal document open | p95 at or below 250 ms | Harness implemented; reference run pending |
| Idle process-tree private working set | Below 150 MB after 60 seconds | Harness implemented; reference run pending |
| Durable recovery lag | At most 2 seconds | Deterministic autosave tests implemented; process-termination harness pending |
| Stress integrity | No crash or unintended byte change | 1,000-file reconciliation and 10,000-edit tests implemented |

No result is marked as passing until raw samples from the reference profile are retained. Hosted CI results may be useful comparisons, but they do not replace the release profile because virtual-machine load and WebView2 versions vary.

The manually dispatched `Release performance` GitHub Actions workflow runs the same gate on a clean Windows hosted runner and retains its JSON output for 30 days. Release sign-off repeats it on the documented reference machine.

### Hosted-run evidence

Commit `212d621` was measured on a four-logical-processor Windows Server 2025 GitHub runner with 16 GB RAM. The retained artifact is `cairn-release-performance-212d62150538a393c97e3701f8f114a0e2ae9beb`.

| Metric | p50 | p95 | Maximum | Result |
|---|---:|---:|---:|---|
| Startup | 1439.1 ms | 3298.5 ms | 8303.9 ms | Failed hosted comparison |
| Document open | 61.5 ms | 78.7 ms | 80.6 ms | Passed |
| Input to painted frame | 31.3 ms | 32.1 ms | 32.4 ms | Passed |
| Idle aggregate working set (pre-correction) | 339.5 MB | n/a | n/a | Diagnostic only |

This run is comparison evidence, not reference-profile sign-off. Its startup stopwatch included post-ready PowerShell diagnostics, and its memory value summed shared pages once per WebView2 process. Both measurement errors are corrected in the current harness. The run also uses the dedicated editor harness rather than the complete production workspace, so complete-workspace startup, idle memory, and stress integrity remain open release-gate work.

## Startup and memory measurement

Build the release binary, then run at least 20 independent launches:

```powershell
npm run tauri build
npm run perf -- "src-tauri\target\release\cairn-md.exe" --samples 40 --idle-seconds 60 --output "performance-results\release.json"
```

Each run uses isolated app metadata and never opens the user's library. The harness measures 40 independent launches from process creation until a real Milkdown editor has painted and accepted input, stops the startup clock before gathering diagnostic process data, closes the full process tree between startup samples, records 20 real visual-editor opens and edits through the next painted frame, and measures the sum of the process tree's private working sets after the idle interval. Forty startup samples make the p95 estimate less sensitive to one-off hosted-runner provisioning noise while retaining every valid sample. The raw report also records aggregate working set for diagnosis. A non-zero exit means at least one enforced threshold failed.

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
