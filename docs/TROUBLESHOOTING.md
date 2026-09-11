# Troubleshooting Cairn.md

## Windows blocks the installer

The v0.1.0 installer is unsigned and may trigger Microsoft Defender SmartScreen. Verify that the installer came from this repository's GitHub release and that its SHA-256 checksum matches the accompanying `.sha256` file. Do not bypass a warning for a file from another source. A future broadly promoted release should be signed before distribution.

## Cairn.md says WebView2 is unavailable

The NSIS installer includes Microsoft's Evergreen WebView2 bootstrapper. Internet access is required only when Windows does not already have a suitable runtime and the bootstrapper must obtain it. Install or repair Microsoft Edge WebView2 Runtime, then launch Cairn.md again.

## The library opens read-only

Cairn.md fails closed when the selected root is unavailable, replaced, unsafe, or no longer supports required durable file operations.

- If you moved the complete library, select **Reconnect library**, choose its new folder, review the match count, and confirm.
- If a network or synchronized drive is temporarily unavailable, restore access before retrying.
- If permissions changed, restore read/write access for your Windows account. Cairn.md does not require administrator access for a normal local library.
- Do not place the library behind a symlink or Windows junction.

Do not rebuild or delete app metadata while recovery is pending.

## A document says Save failed

Stop and preserve both copies.

- If access was temporarily denied, correct it and select **Retry**.
- If another application changed the file, choose either the latest external file or **Save as recovered copy**.
- If the file or project was deleted externally, the recovery panel remains available and can recreate the project folder for the recovered copy.

Cairn.md never silently retries a local draft against a new external fingerprint.

## Markdown appears as a source-only block

The block contains syntax that Visual mode cannot safely round-trip. Select **Edit in Source** to change the exact Markdown. This is preservation behavior, not file corruption.

## A tracked folder cannot be added

Tracked folders cannot be the library root, live inside it, or contain it. Choose a separate source folder. Tracking remains read-only; only an explicit import creates a managed copy.

## A local Rust build cannot find link.exe

Visual Studio Code does not include Microsoft's native linker. Install Visual Studio 2022 Build Tools with **Desktop development with C++**, the MSVC toolchain, and a Windows SDK. Restart the terminal so the Rust build can discover the tools.

## Reporting a problem

Include the Cairn.md version or commit, Windows version, the action you attempted, and the exact error text. Never attach confidential Markdown or publish private file paths. Recovery databases may contain document drafts, so do not upload them to a public issue.
