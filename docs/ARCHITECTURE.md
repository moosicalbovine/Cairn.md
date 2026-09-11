# Cairn.md architecture

## System shape

Cairn.md is a Windows-first Tauri 2 desktop application. React and TypeScript render the WebView2 interface; Rust owns filesystem access, SQLite metadata, imports, recovery, and protected saves. The frontend receives narrow typed snapshots and byte arrays through Tauri commands rather than general filesystem access.

The canonical user content is always the `.md` file beneath the selected library root. SQLite is private app state, not a second document store, except for short-lived durable recovery snapshots needed to prevent data loss.

## Storage model

One user-selected library root contains direct child project folders. In v0.1.0, each project contains direct child Markdown files. Stored project and document paths are relative to the library root, so moving the complete folder does not rewrite every record.

The SQLite database stores:

- the library identity and binding generation;
- project and document IDs, relative paths, file identities, and disk fingerprints;
- import source path and import date;
- tracked-folder references;
- durable operation-journal entries; and
- recovery snapshots and external-conflict evidence.

The database runs in WAL mode with foreign keys and durable synchronization. Migrations are forward-only. Failed integrity or schema checks preserve evidence and open the library read-only instead of silently replacing metadata.

## Filesystem safety

Every mutation is resolved beneath the verified library root. Traversal, absolute managed paths, links, and Windows reparse-point escapes are rejected. Tracked folders are read-only sources and cannot overlap the managed library.

Operations spanning disk and SQLite use a journal with these phases:

1. Intent recorded
2. Temporary durable
3. Filesystem finalized
4. Metadata committed
5. Cleanup complete

Startup replay advances an interrupted operation exactly once and removes only operation-owned artifacts whose identity and fingerprint match the journal.

Imports copy to a same-directory temporary file, verify the source before and after copying, finalize without overwriting, and commit provenance only after the destination is durable. Saves hold a Windows handle that denies competing writers while permitting atomic replacement, use an operation-owned backup, verify the final hash, and only then advance metadata.

## Editing and Markdown preservation

`MarkdownSession` owns the exact canonical source, line-ending convention, UTF-8 BOM state, and monotonic revision. CodeMirror 6 edits that source directly in Source mode.

Visual mode uses Milkdown 7 over contiguous supported CommonMark and GitHub Flavored Markdown regions. Unsupported HTML, front matter, math-like syntax, and ambiguous blocks stay source-backed. Visual transactions patch only the mapped source range. Stable region IDs and projection subscriptions keep multiple mounted regions on the same current revision.

Large supported documents are divided at top-level heading boundaries into
section-sized source ranges. The first editable section mounts immediately;
later sections reserve layout space and initialize as they approach the scroll
viewport. Small documents retain one visual editing region.

Invalid UTF-8 files open read-only and retain their original bytes. Opening, switching modes, and closing without edits are byte-preserving operations.

## Autosave and recovery

Each writable document has one serial autosave controller and three revisions:

- current revision acknowledged by the editor;
- durable snapshot revision stored in SQLite; and
- disk revision proven in the Markdown file.

Recovery uses a bounded cadence during continuous typing. Document and project transitions await a durable recovery barrier before releasing the active session. The Windows close-request handler applies the same barrier before destroying the app window and cancels exit if recovery storage is unavailable. Disk saves are coalesced, with at most one save in flight. **Saved** appears only when disk and editor revisions match.

Watcher events are hints. Rust performs an authoritative scan before updating the index. A clean active session reloads an external change. If local work is pending, the protected save records both sides, stops autosave, and requires explicit reload or recovered-copy resolution. Reconciliation never cascade-deletes a pending recovery record when an external file or project disappears.

## Startup and performance

Startup reads the cached SQLite index and tracked folders in parallel, then makes the workspace interactive. Full filesystem reconciliation runs in the mounted workspace instead of blocking the splash screen. Workspace, setup, Milkdown, and CodeMirror code are loaded as separate lazy chunks.

The release benchmark launches isolated app instances, waits for a real Milkdown ready signal, records startup, document-open, edit-to-paint, and full process-tree memory samples, then applies the thresholds in [PERFORMANCE.md](PERFORMANCE.md).

## Version boundaries

- v0.1.0: one library root, top-level projects, imports, tracked folders, dual-mode editing, themes, autosave, recovery, and external-change protection.
- v0.2.0: nested sub-projects, tags, fuzzy and ranked search, filters, and organized views.
- v1.0.0: one local Git repository per top-level project, named versions, simple committed/uncommitted state, history navigation, and diffs.
