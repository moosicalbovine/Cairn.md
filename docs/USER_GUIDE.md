# Cairn.md User Guide

> [!NOTE]
> Cairn.md is under active `v0.1.0` development. Screens and wording may still change before the first signed release.

## What Cairn.md is for

Cairn.md is a local Windows workspace for Markdown documents that arrive through email, Microsoft Teams, SharePoint folders, AI tools, downloads, and other applications.

It is designed to help you:

- collect Markdown files into one dependable library;
- organize them into projects;
- read and edit them without returning to the application where they arrived;
- keep the resulting `.md` files clean and easy to share; and
- recover work safely if the application or computer stops unexpectedly.

## Core concepts

### Library folder

The library folder is a location you choose on your PC. Cairn.md stores its project folders and Markdown files beneath this folder.

Cairn.md records project and document locations relative to the library folder. You can move the entire library and reconnect it from Settings without breaking its internal structure.

### Projects

In `v0.1.0`, a project is a top-level folder used to group related Markdown files. Nested sub-projects are planned for `v0.2.0`.

### Tracked PC folders

Tracked PC folders provide quick, read-only access to places where Markdown files frequently arrive, such as Downloads or a locally synchronized SharePoint folder.

Tracking a folder does not move, copy, or modify anything. A source file enters the Cairn.md library only when you explicitly import it.

### Imported copies

Every import creates a new, independent copy inside the selected project. Cairn.md never edits the tracked original.

The app privately records the original path and import date for reference. This metadata is not inserted into the Markdown file and is not included when you share that file.

Importing the same source more than once creates another copy. If a filename already exists in the destination project, Cairn.md chooses an available name such as `proposal (2).md`; you can rename it afterward.

## v0.1.0 workflow

### 1. Choose or reconnect a library

On first launch:

1. Select an existing empty folder or create a folder for the Cairn.md library.
2. Cairn.md validates that it can read and write there.
3. The app opens the library workspace.

If you later move the library folder, Cairn.md opens the cached library read-only. Select **Reconnect library**, choose the new location, review the project and document match count, and confirm the new binding.

### 2. Create a project

1. Select **New project**.
2. Enter a project name.
3. Cairn.md creates the project under the library folder and opens it.

### 3. Import Markdown files

You can import in three ways:

- select **Import files** and choose one or more `.md` files;
- drag `.md` files into a project; or
- expand **Tracked PC folders**, browse to a source file, and select **Import into project**.

Each selected file becomes an independent library copy. The source remains unchanged.

### 4. Read and edit a document

Opening a document keeps three areas available:

1. **Library navigation** for projects and quick-access views.
2. **Project contents** for nearby files in the selected project.
3. **Document workspace** for reading and editing the selected file.

The Project contents pane can be resized or collapsed when you want more writing space.

#### Visual mode

Visual mode is the default. You edit the rendered document directly using familiar formatting controls and keyboard shortcuts. Cairn.md saves the result as standard Markdown.

#### Source mode

Source mode exposes the underlying Markdown text for precise editing or syntax that cannot be represented safely in Visual mode.

There is no separate Preview or Split mode in `v0.1.0` because Visual mode is already an editable rendered view.

### 5. Autosave and recovery

Autosave is always enabled. The normal document indicator is **Saved**; brief **Saving…** feedback may appear while a disk write completes.

The only persistence labels are **Saving…**, **Saved**, **Save failed**, and **Recovered**. If saving fails, use **Retry** after correcting the problem. If the file changed outside Cairn.md, autosave stops and preserves both the local draft and the external file. You can reload the latest external file or save the draft as a separately numbered recovered copy.

After an unexpected shutdown, Cairn.md offers the latest durable editing snapshot. Continuous typing does not postpone recovery snapshots. Switching documents or projects and closing the app wait for the acknowledged revision to reach recovery storage; if that write fails, Cairn.md keeps the current document or window open and shows the error.

## Markdown compatibility

Cairn.md targets CommonMark plus widely adopted GitHub Flavored Markdown features, including:

- headings and paragraphs;
- emphasis and strikethrough;
- links and images;
- ordered and unordered lists;
- task lists;
- block quotes;
- tables;
- inline code and fenced code blocks; and
- autolinks.

Cairn.md does not require proprietary markup. Unsupported or ambiguous syntax must be preserved rather than silently discarded.

## Appearance

`v0.1.0` provides three appearance settings:

- **Light**
- **Dark**
- **Follow Windows**

Changing the appearance never changes document content.

## Privacy and data ownership

- Documents remain in the library folder you select.
- Imported originals are never edited by Cairn.md.
- Application metadata remains local and is not inserted into Markdown files.
- No account or cloud service is required.
- Moving or sharing a `.md` file shares only its document content.

Back up the chosen library folder to protect the portable Markdown files. Cairn.md keeps its private SQLite index and recovery records in its Windows app-local-data directory; include that app data in a full disaster-recovery backup. The library remains usable as ordinary folders and `.md` files even without the private metadata.

## SemVer roadmap

### v0.2.0: sub-projects and retrieval

- Nested sub-projects
- Tags used for search and organized views
- Fuzzy matching for filenames, titles, tags, projects, and paths
- Ranked full-text search inside documents
- Filters for projects, tags, dates, and source folders
- Saved searches that organize results without moving or duplicating files

### v1.0.0: public launch with local history

- One local Git repository per top-level project
- Manual named versions
- Automatic recovery checkpoints
- A simple committed or uncommitted indicator with a shortened commit hash
- Navigation through older committed versions of a document
- Read-only viewing of a selected historical version
- A readable diff between the current document and a selected older version

Developer-oriented Git concepts such as staging, branches, remotes, pushing, pulling, and merging will remain hidden.

## Performance and reliability targets

`v0.1.0` is intended to meet these targets on a typical work laptop:

- cold launch within 1.5 seconds;
- editor input response under 50 milliseconds;
- open a normal Markdown file within 250 milliseconds;
- idle memory usage below 150 MB;
- autosave without visible typing interruption; and
- no crash or content corruption during normal import, editing, theme switching, or project management.

## Getting help

Until a supported build is published, use this GitHub repository's issue tracker for product questions, workflow examples, documentation feedback, and accessibility concerns. Never attach confidential workplace documents or expose private file paths in a public issue.
