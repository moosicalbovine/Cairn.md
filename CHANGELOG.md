# Changelog

All notable Cairn.md changes are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-09-14

### Fixed

- Startup failures now show the available diagnostic detail and clear recovery
  instructions.
- Repository and user guidance now match the implemented MVP, release status,
  paths, and measured performance contract.

### Changed

- Installer validation now exercises an upgrade from the previous release and
  confirms that the existing library remains usable.
- Historical scope notes identify later reference corrections without changing
  the original product decisions.

## [0.1.0] - 2026-09-11

### Added

- A Windows-first local Markdown library with one user-selected, relocatable root.
- Top-level projects and persistent contents navigation beside the editor.
- File-picker, drag-and-drop, and import-only tracked-folder workflows.
- Direct visual Markdown editing with formatting controls and a Source fallback.
- CommonMark and GitHub Flavored Markdown support with source-backed preservation
  for unsupported or ambiguous syntax.
- Always-on autosave, durable recovery, external-change protection, and safe
  recovered copies.
- Light, dark, and Follow Windows appearance modes.
- Current-user NSIS packaging with the Evergreen WebView2 bootstrapper.
- Automated Markdown preservation, stress, forced-termination, desktop, installer,
  and release-performance checks.

[0.1.1]: https://github.com/moosicalbovine/Cairn.md/releases/tag/v0.1.1
[0.1.0]: https://github.com/moosicalbovine/Cairn.md/releases/tag/v0.1.0
