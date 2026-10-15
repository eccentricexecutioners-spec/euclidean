# Updates through Cargo Bay

Euclidean keeps itself up to date through Cargo Bay, the same way the Eccentric Executioners
Fusion add-ins do. The client is `crates/uplink` (package `vectorcraft-uplink`), a Rust port of
cargo-bay's `clients/fusion-uplink/uplink/`. The app wires it in from `apps/vectorcraft/src/main.rs`;
the UI side (dialog, menu items, status line) is `crates/ui-egui/src/dialogs/cargo_bay.rs`, which
needs no network code of its own.

## The one host

Installed copies talk only to `https://cargo-bay.eccentricexecutioners.ca`. Two calls, one attempt
each, 10 s, no retry loop:

| Call | Body | Answer |
|---|---|---|
| `POST /api/v1/installs/claim` (no auth) | `{link_code, tool_id: "euclidean", platform}` | `{install_key: "cbk_…"}`; 404 bad code, 403 not granted, 429 rate limited |
| `POST /api/v1/update-check` (`Authorization: Bearer cbk_…`) | `{tool_id, installed_version, channel: "stable", platform, keyring_serial}` | `up_to_date`, `not_granted`, `unknown_tool`, or `update` with a signed manifest and a 10-minute download URL; 401 / `bad_key` / `key_tool_mismatch` retire the key |

`platform` is `mac`, `windows` or `linux`. The version sent is the workspace version
(`[workspace.package] version`, set with `cargo xtask version set`), which starts at 0.1.0.

## What is kept on disk

Everything lives in one folder, shared with the add-ins:
macOS `~/Library/Application Support/EE Cargo Bay/`, Windows `%APPDATA%\EE Cargo Bay\`.

| Path | What |
|---|---|
| `euclidean.json` | `{tool_id, install_key, base_url, linked_at}`, mode 0600, written atomically |
| `euclidean.revoked-<stamp>.json` | a key Cargo Bay refused (renamed, never deleted) |
| `euclidean.keyring.json` | the newest master-signed key list, `{document, signature}` |
| `staging/euclidean-<version>/` | the downloaded zip, its `manifest.json`, and the unzipped `Euclidean.app` |
| `pending/euclidean.json` | a staged update waiting for the next start |
| `pending/euclidean.done-<stamp>.json` | an update that was swapped in; `pending/euclidean.shown.json` remembers it was announced |
| `pending/euclidean.stale-<stamp>.json` | a staged update that was not newer than the running copy, set aside |
| `backups/Euclidean-<old>-<stamp>.app` | the copy an update replaced |

Nothing in the updater deletes anything; old files are renamed or moved aside.

## Trust

A release manifest is Ed25519-signed over its canonical JSON (keys sorted by UTF-16 code unit, no
whitespace, strings as JavaScript's `JSON.stringify` writes them). The app trusts the Cargo Bay
master public key (built in, `crates/uplink/src/trust.rs`, the same value the add-ins carry), then
the release keys in a key list the master key signed. A key list is kept only when its serial is
higher than the one held.

Before an update is staged: signature, then size, then SHA-256, then the zip's entries (exactly
one top-level `Euclidean.app/`, no absolute paths, no `..`, no backslashes, symlinks only when
they stay inside the bundle). At the next start the staged bundle is checked again: the zip's
signature, size and SHA-256, and the unzipped tree against the zip (every file's size, CRC-32 and
executable bit; every symlink's target; nothing extra).

## The swap (macOS)

At start, before any window, if `pending/euclidean.json` exists and its version is newer than the
running one:

1. Verify it again (above).
2. Move the running bundle (`current_exe()`, three levels up) to `backups/`.
3. Move the staged bundle into its place.
4. Rename the pending file to `done-<stamp>`.
5. `open -n` the new bundle and exit.

If a move fails, the old bundle is moved back and the pending file is kept; the status line says
so. The downloader uses plain file I/O and never sets `com.apple.quarantine`, and the app's
`Info.plist` does not set `LSFileQuarantineEnabled`, so a swapped-in copy is not quarantined.

Windows and Linux check only: an update shows "{new} is available — download it from the
Mothership's Tools page".

## In the app

- First start with no key: the **Link to Cargo Bay** dialog (code `XXXX-XXXX`, case-insensitive,
  dash optional). **Later** closes it until the next start.
- **Check for Updates…** (`help.checkForUpdates`; macOS app menu, else Help) and **Link to Cargo
  Bay…** (`help.linkCargoBay`, Help). Both are UI commands, so the CLI and control channel reach
  them.
- A linked copy checks once at start on a background thread. A staged update shows "{new} ready"
  and **Restart to update** in the status line; the button quits as Quit does (unsaved documents
  are asked about) and opens the app again, which swaps at start.

## Off switch

The updater does nothing (no network, no dialog) when any of these holds:
`EUCLIDEAN_NO_UPDATE=1`; `VECTORCRAFT_NO_PREFS` set (screenshot and test runs); a headless run
(the CLI and MCP do not link the crate); the binary is not inside a `.app` bundle (a `target/` dev
run; elsewhere, a path through a `target` folder).

## Testing

- `cargo test -p vectorcraft-uplink` — state, canonical JSON against cargo-bay's own vectors
  (`crates/uplink/tests/fixtures/`), trust, key lists, zip rules, claim and check against a
  `127.0.0.1` server, staging, and the swap on bundle copies in a temp folder.
- `cargo test -p vectorcraft-ui-egui --lib cargo_bay` — the dialog, menu commands and status line
  with a fake host.
- End to end: build with `--features uplink-test-key`, which adds a TEST master key (never in a
  release build; a test-key binary contains the string `uplink-test-key-marker`). Point a bundle
  copy at a local server with `EUCLIDEAN_CARGO_BAY_URL=http://127.0.0.1:<port>`, keep its state
  out of the real folder with `EUCLIDEAN_CARGO_BAY_STATE_DIR=<scratch>` and its preferences with
  `VECTORCRAFT_CONFIG_DIR=<scratch>`, and serve a zip whose manifest is signed with the test key.
  The relaunch passes those variables on (`open --env`).
