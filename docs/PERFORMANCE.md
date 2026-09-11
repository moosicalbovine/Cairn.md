# Cairn.md performance contract

Cairn.md v0.1.0 targets a responsive Windows desktop experience without bundling a browser runtime. Release candidates are measured from an x64 release build on a documented reference Windows profile.

## Release thresholds

| Metric | Required result | Current evidence |
|---|---:|---|
| Cold start to an interactive editor | p95 at or below 1.5 seconds | 1,308.0 ms p95; passed hosted release gate |
| Editor input to painted frame | p95 at or below 50 ms | 32.0 ms p95; passed hosted release gate |
| Normal document open | p95 at or below 250 ms | 139.3 ms p95; passed hosted release gate |
| Idle process-tree private working set | Below 150 MB after 60 seconds | 74.0 MB; passed hosted release gate |
| Durable recovery lag | At most 2 seconds | Forced-termination desktop flow passed in CI |
| Stress integrity | No crash or unintended byte change | 1,000-file reconciliation and 10,000-edit tests passed in CI |

No result is marked as passing until raw samples from the reference profile are retained. Hosted CI results may be useful comparisons, but they do not replace the release profile because virtual-machine load and WebView2 versions vary.

The manually dispatched `Release performance` GitHub Actions workflow runs the same gate on a clean Windows hosted runner and retains its JSON output for 30 days. Release sign-off repeats it on the documented reference machine.

### Hosted-run evidence

Commit `b381ae6` passed the release comparison in [GitHub Actions run 34600493682](https://github.com/moosicalbovine/Cairn.md/actions/runs/34600493682). The retained artifacts are `cairn-release-performance-b381ae6061351a2a04bc1db608ca7fa57e21f0d5` and `cairn-md-validated-installer-b381ae6061351a2a04bc1db608ca7fa57e21f0d5`.

The runner used Windows Server 2025 Datacenter `10.0.26100`, WebView2 `152.0.4191.66`, four logical processors from an AMD EPYC 9V45 host, 16 GB RAM, and SSD storage. Each startup sample used an isolated app-data directory. The harness waited one second after terminating each complete WebView2 process tree so rapid test relaunches did not overlap Windows process and antimalware cleanup.

| Metric | p50 | p95 | Maximum | Result |
|---|---:|---:|---:|---|
| Startup, 40 samples | 1,069.7 ms | 1,308.0 ms | 8,598.8 ms | Passed |
| Document open, 20 samples | 109.7 ms | 139.3 ms | 230.6 ms | Passed |
| Input to painted frame, 20 samples | 31.3 ms | 32.0 ms | 32.2 ms | Passed |
| Idle private working set after 60 seconds | n/a | n/a | 74.0 MB | Passed |

Two earlier `e6f1fef` comparison runs are retained rather than discarded: [run 34597407797](https://github.com/moosicalbovine/Cairn.md/actions/runs/34597407797) recorded startup at 4,127.9 ms p95, and [run 34598812964](https://github.com/moosicalbovine/Cairn.md/actions/runs/34598812964) recorded 5,556.4 ms p95. In both runs, normal starts clustered near 1.0-1.5 seconds before a block of rapid relaunches slowed to 3.3-5.8 seconds. Document open, input latency, and private memory passed in both. The harness now implements the plan's required reset interval between samples; it does not remove or replace any slow sample inside a run.

Hosted results are release-gate evidence, but they are not a dedicated physical reference-machine benchmark because virtual-machine load varies. A local reference run is still desirable when a Windows machine with the Visual Studio C++ workload is available.

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
