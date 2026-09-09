# Cairn.md

Cairn.md is a lightweight Windows desktop application for collecting, organizing, reading, and editing Markdown files locally.

> [!IMPORTANT]
> Cairn.md is under active development at `v0.1.0`. There is no supported installer yet.

## Why Cairn.md?

Markdown files increasingly arrive through email, Microsoft Teams, SharePoint folders, and AI tools. Native viewers make them easy to read once, but difficult to rediscover, organize, edit, and version later.

Cairn.md will provide a personal managed library that keeps Markdown files easy to find and work with while preserving clean, portable `.md` files.

## v0.1.0 — MVP (current)

- Windows-first desktop experience
- One user-selected, relocatable library folder
- Top-level projects for organizing documents
- File-picker and drag-and-drop imports
- Collapsible tracked PC folders for import-only access
- Visual Markdown editing with a Source fallback
- Always-on autosave and crash recovery
- CommonMark and GitHub Flavored Markdown support
- Light, dark, and Follow Windows themes
- Local-only operation with no required account or cloud service

## SemVer roadmap

### v0.2.0

- Nested sub-projects
- User-defined tags
- Fuzzy search across filenames and metadata
- Ranked full-text content search
- Metadata filters and organized search views

### v1.0.0 — first public announcement

- One local Git repository per top-level project
- Manual named versions and automatic recovery checkpoints
- A minimal Git status of committed or uncommitted changes with a short commit hash
- Navigation through older committed versions of a document
- Read-only historical viewing and current-to-history diffs

## Product principles

- **Local by default:** documents and application metadata remain on the user's PC.
- **Portable Markdown:** Cairn.md never requires proprietary syntax inside `.md` files.
- **Safe imports:** importing always creates an independent library copy and never edits the tracked original.
- **Fast and dependable:** startup, typing, autosave, and recovery are treated as core product requirements.
- **Simple versioning:** Git powers local history without exposing developer-oriented workflows.

## Documentation

- [Planned user guide](docs/USER_GUIDE.md)
- [Contributing](CONTRIBUTING.md)

## Contributing

Cairn.md is being built in the open. Ideas, accessibility feedback, workflow examples, and code contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before contributing.

## License

Cairn.md is licensed under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for attribution information.
