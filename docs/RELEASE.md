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
npm run test:flows
npm run tauri build -- --debug --no-bundle --config src-tauri/tauri.webdriver.conf.json
npm run test:desktop
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
npm run release:verify-webview2 -- -InstallerPath $installer.FullName
```

The script installs for the current user and runs an isolated workflow through the installed release binary: create a project, import from a tracked folder, visually edit Markdown, inspect the same edit through Source mode, and persist it through the recovery-backed save path. It verifies that the tracked original stays unchanged, performs a same-version reinstall, uninstalls silently, and confirms the user-selected library remains.

The release workflows run that installed-app test with Evergreen WebView2 present. They also inspect Tauri's generated NSIS program and the final installer to prove that the missing-runtime branch installs the embedded bootstrapper. The check requires the bootstrapper in the installer to be byte-for-byte identical to the Microsoft-signed build input.

GitHub's current Windows runners use Windows Server, where Microsoft installs WebView2 as a required component and does not support removing it. Those runners therefore cannot execute the missing-runtime branch. For a prerelease, the package proof above is accepted with this limitation disclosed. A stable release still requires an install test on a clean Windows client without WebView2.

CI retains ordinary unsigned installer artifacts for 14 days and release evidence for 90 days. Signing is conditional on a maintainer-provided certificate and timestamp service; secrets must never be committed. Every release must clearly say whether its installer is signed.

## Performance evidence

Dispatch the **Release performance** workflow or run the command in [PERFORMANCE.md](PERFORMANCE.md) on the reference machine. Retain raw JSON, environment details, and the exact commit. Every threshold must pass; do not replace a slow valid sample.

## Installer validation checklist

Complete this from a clean Windows sandbox before a stable release. A prerelease may carry an unchecked item only when the release notes state the limitation and an automated package-level substitute is green.

- [ ] Install for the current user without administrator credentials.
- [ ] Launch with an existing Evergreen WebView2 runtime.
- [ ] Launch on a Windows client snapshot without WebView2 and verify that the embedded bootstrapper installs it. The v0.1.0 prerelease has package-level proof; hosted execution is unavailable because GitHub's Windows Server image cannot remove WebView2.
- [ ] Disconnect networking after prerequisites are installed and complete create, import, edit, autosave, recovery, and reconnect flows.
- [ ] Upgrade from the previous released version and preserve library binding, provenance, and recovery metadata. Not applicable to v0.1.0, which has no predecessor.
- [ ] Attempt a downgrade and confirm it is blocked.
- [ ] Uninstall and confirm the user-selected library folder and Markdown files remain untouched.
- [ ] Reinstall and reconnect the retained library.
- [ ] Verify the installer and installed executable signatures when signing is enabled. Not applicable to the explicitly unsigned v0.1.0 preview.

## Release gate

Do not tag or publish v0.1.0 until:

- CI static checks, tests, Markdown corpus, and release build are green;
- the release performance report passes;
- the installer checklist is complete on the reference Windows profile, or any prerelease exception is documented and backed by an automated package-level check;
- user, architecture, performance, release, and troubleshooting documentation matches shipped behavior; and
- the Git worktree contains only intentional committed changes.
