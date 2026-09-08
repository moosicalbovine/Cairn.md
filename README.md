# NoteMD

NoteMD is a planned lightweight Windows desktop application for collecting, organizing, reading, and editing Markdown files locally.

> [!IMPORTANT]
> NoteMD is currently in the specification phase. There is no installable application yet.

## Why NoteMD?

Markdown files increasingly arrive through email, Microsoft Teams, SharePoint folders, and AI tools. Native viewers make them easy to read once, but difficult to rediscover, organize, edit, and version later.

NoteMD will provide a personal managed library that keeps Markdown files easy to find and work with while preserving clean, portable `.md` files.

## Planned MVP

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

## Planned roadmap

### Version 2

- Nested sub-projects
- One local Git repository per top-level project
- Manual named versions and automatic recovery checkpoints
- A minimal Git status of committed or uncommitted changes with a short commit hash

### Version 3

- User-defined tags
- Fuzzy search across filenames and metadata
- Ranked full-text content search
- Metadata filters and organized search views

## Product principles

- **Local by default:** documents and application metadata remain on the user's PC.
- **Portable Markdown:** NoteMD never requires proprietary syntax inside `.md` files.
- **Safe imports:** importing always creates an independent library copy and never edits the tracked original.
- **Fast and dependable:** startup, typing, autosave, and recovery are treated as core product requirements.
- **Simple versioning:** Git powers local history without exposing developer-oriented workflows.

## Documentation

- [Planned user guide](docs/USER_GUIDE.md)
- [Contributing](CONTRIBUTING.md)

## Contributing

NoteMD is being designed in the open. Ideas, accessibility feedback, workflow examples, and future code contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before contributing.

## License

NoteMD is licensed under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for attribution information.

