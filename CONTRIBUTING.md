# Contributing to Cairn.md

Thank you for helping make local Markdown work easier and safer.

## Project status

Cairn.md is under active `v0.1.0` development. Contributions are most useful when they improve implementation quality, tests, requirements, accessibility, workflows, or documentation.

## Ways to contribute

- Describe a real Markdown workflow and where existing tools create friction.
- Report an accessibility or usability concern.
- Suggest a focused improvement to the product specification.
- Improve documentation, examples, or terminology.
- Contribute focused code changes with tests and documentation where behavior changes.

## Contribution guidelines

1. Keep changes focused on one outcome.
2. Explain the user problem or maintenance benefit in the pull request.
3. Preserve clean, portable CommonMark and GitHub Flavored Markdown.
4. Avoid introducing proprietary syntax into user documents.
5. Add or update verification when a change affects behavior.
6. Do not include confidential workplace files, paths, or content in issues, tests, or examples.

## Commit messages

Use concise conventional commits when practical, for example:

```text
docs: clarify the import workflow
feat(editor): add visual task-list editing
fix(library): preserve projects after moving the library root
```

## Local development on Windows

Install:

- Node.js 24 or newer;
- the stable Rust toolchain with `rustfmt` and `clippy`;
- Visual Studio 2022 Build Tools with **Desktop development with C++**, including the MSVC linker and Windows SDK; and
- Microsoft Edge WebView2 Runtime.

Visual Studio Code is a useful editor but does not include the MSVC linker. If Rust reports that `link.exe` is missing, add the Visual Studio C++ workload before retrying.

From the repository root:

```powershell
npm ci
npm run lint
npm run typecheck
npm test -- --run
npm run test:flows
npm run test:markdown
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
npm run tauri build
```

`test:flows` is the fast mocked integration suite. The real desktop smoke flow is
Windows-only and additionally requires Microsoft Edge WebDriver matching the
installed WebView2 Runtime on `PATH`. Build and run it with:

```powershell
npm run tauri build -- --debug --no-bundle --config src-tauri/tauri.webdriver.conf.json
npm run test:desktop
```

CI downloads the matching Edge WebDriver automatically. The desktop harness
drives its W3C protocol directly and uses isolated temporary library, app-data,
and WebView2 profile directories. It never opens the user's configured Cairn.md
library or production WebView2 profile.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before changing persistence or filesystem behavior. Those paths fail closed intentionally and require fault-injection coverage.

## Licensing contributions

Unless explicitly stated otherwise, contributions intentionally submitted to Cairn.md are licensed under the Apache License 2.0, as described in the repository's [LICENSE](LICENSE).
