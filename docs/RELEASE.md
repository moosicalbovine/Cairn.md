# Cairn.md release process

## Version policy

Cairn.md follows Semantic Versioning. v0.1.0 is the top-level-project MVP, v0.2.0 adds hierarchy and retrieval, and v1.0.0 is the first publicly announced release with local Git history.

Keep the version synchronized in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`.

## Build

The supported target is 64-bit Windows. Install the prerequisites in [CONTRIBUTING.md](../CONTRIBUTING.md), then run:

```powershell
npm ci
npm run lint
npm run typecheck
npm test -- --run
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
npm run test:markdown
npm run build
npm run tauri build
```

The Tauri build produces an x64 current-user NSIS installer beneath `src-tauri/target/release/bundle/nsis`. It embeds the small Evergreen WebView2 bootstrapper, blocks downgrades, and does not include an auto-updater.

Smoke-test that installer on a disposable Windows account or CI runner:

```powershell
$installer = Get-ChildItem "src-tauri\target\release\bundle\nsis\*.exe" | Select-Object -First 1
$version = (Get-Content "src-tauri\tauri.conf.json" | ConvertFrom-Json).version
npm run release:validate -- -InstallerPath $installer.FullName -ExpectedVersion $version
```

The script installs for the current user, launches the installed editor with isolated app data, waits for the real editor-ready signal, uninstalls silently, and confirms a user-library sentinel remains. It does not replace the clean-sandbox WebView2-present and WebView2-absent checklist below.

CI retains unsigned installer artifacts for 14 days. Signing is conditional on a maintainer-provided certificate and timestamp service; secrets must never be committed. A public release must clearly say whether its installer is signed.

## Performance evidence

Dispatch the **Release performance** workflow or run the command in [PERFORMANCE.md](PERFORMANCE.md) on the reference machine. Retain raw JSON, environment details, and the exact commit. Every threshold must pass; do not replace a slow valid sample.

## Installer validation checklist

Complete this from a clean Windows sandbox before release:

- [ ] Install for the current user without administrator credentials.
- [ ] Launch with an existing Evergreen WebView2 runtime.
- [ ] Launch on a snapshot without WebView2 and verify the embedded bootstrapper path.
- [ ] Disconnect networking after prerequisites are installed and complete create, import, edit, autosave, recovery, and reconnect flows.
- [ ] Upgrade from the previous released version and preserve library binding, provenance, and recovery metadata.
- [ ] Attempt a downgrade and confirm it is blocked.
- [ ] Uninstall and confirm the user-selected library folder and Markdown files remain untouched.
- [ ] Reinstall and reconnect the retained library.
- [ ] Verify the installer and installed executable signatures when signing is enabled.

## Release gate

Do not tag or publish v0.1.0 until:

- CI static checks, tests, Markdown corpus, and release build are green;
- the release performance report passes;
- the installer checklist is complete on the reference Windows profile;
- user, architecture, performance, release, and troubleshooting documentation matches shipped behavior; and
- the Git worktree contains only intentional committed changes.
