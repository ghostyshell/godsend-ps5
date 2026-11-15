# GODsend PS5: project overview for agents

GODsend PS5 is a desktop workshop for a jailbroken PlayStation 5. It speaks
FTP for files, elfldr for payloads and the Ghostpad payload for the virtual
controller, then wraps them in the GODsend family shell (Rust workspace plus
Tauri 2 plus React).

## Components

- **`core/` (`godsend-core`)**: framework-agnostic engine. No Tauri, no CLI
  parsing.
  - `config`: hosts (FTP, pad, loader and PKG Manager ports), active host,
    download folder, debrid keys, service bases, homebrew and package folders.
  - `ftp`: suppaftp wrapper with port fallback (configured, 2121, 1337, 21),
    tolerant listing (MLSD, Unix LIST, DOS LIST), progress-transfer loops,
    `read_head`, recursive remove, `mkdir_all`. PS5 paths are absolute
    (`/user/...`, `/data/...`, `/mnt/...`).
  - `device`: probing (FTP port, etaHEN config, homebrew folder, Ghostpad,
    elfldr, PKG Manager) and LAN discovery on 2121 and 1337.
  - `ghostpad`: TCP client for the 16-byte `GPAD` frame at 60 Hz. Owns the
    sender thread and the button, stick and trigger state.
  - `pad`: button vocabulary (ScePad bits), tap, hold, release, combos, macros
    and stick directions.
  - `payload`: elfldr sender on 9021.
  - `library`: scan of `/user/app` plus `/user/appmeta` metadata, homebrew
    folders on internal and USB storage, and the package queue. Icon cache
    keyed by path hash.
  - `install`: classification (PKG, image, folder, ELF, ZIP), destination
    layout, ZIP unpacking, FAT32 splitting for USB.
  - `pkg`: PS4/PS5 PKG header inspection, content id and title id.
  - `convert`: folder to ISO, ISO to folder, ZIP pack/unpack, split/merge.
  - `homebrew`: runtime payload catalogue from the ps5-payloads mirror.
  - `sources`, `store`, `debrid`, `aria2`: game index and Internet Archive
    search, link selection, debrid resolution, downloads.
  - `transfer`: JobManager with progress hook, cancellation and a journal.
- **`cli/` (`godsend-ps5`)**: one command per feature area. Run with no
  arguments for the list.
- **`app/`**: Tauri 2 shell. `app/src-tauri/src/lib.rs` has the command layer;
  `app/src` is React 18 plus TypeScript with one hand-rolled `styles.css`
  (see DESIGN.md) and a mock bridge in `mock.ts` behind `?mock=1`.

## Invariants

- `core/` must stay UI-free and process-free. It returns `Result<T>`, and the
  Tauri error serializes as the Display string.
- A console is addressable by FTP; nothing assumes a mounted volume.
- FTP servers disagree about listings and the anonymous user. Try MLSD then
  LIST, `anonymous` then `ftp`, and keep the port that worked.
- PKG installs are never claimed. The app uploads to the folder PS5Upload or
  PKG Manager watches; the message says that, not "installed".
- Images only mount because ShadowMount+ watches `/data/homebrew`. Keep the
  destination configurable and the message specific.
- `GPAD` frames are 16 bytes: magic, button mask big-endian, LX/LY/RX/RY,
  L2/R2, two reserved bytes. 128 is stick centre.
- Destructive operations (delete, overwrite) require an explicit confirmation
  in the UI.
- FTP on a PS5 is plaintext; never send credentials anywhere else and never
  log passwords.

## Building and testing

```sh
cargo test --workspace
cargo check --workspace
npm --prefix app install
npm --prefix app run build          # tsc --noEmit plus vite build
npm --prefix app run dev:mock       # browser UI on 1465, no hardware
npm --prefix app run test:ui        # Playwright against the mock bridge
```

The end-to-end core test, `core/tests/mock_device.rs`, runs a fake FTP server,
a fake Ghostpad, a fake elfldr and a fake PKG Manager, then drives scan, icon
fetch, PKG upload, folder upload, payload send, pad frames and probing. Extend
it, not just unit tests, when a flow changes.

The dev server uses port 1465, not Vite's 1420, because sibling GODsend apps
run on 1420 and the Playwright suite must test this app.

### macOS overlay title bar

The window uses `titleBarStyle: "Overlay"` with a hidden title, so content
draws under the traffic lights. `app/src/main.tsx` sets
`document.documentElement.dataset.os = "mac"` (only inside the real app, or
with `?os=mac` for a preview). The stylesheet then uses an 84px topbar row
with `padding-top: 22px`. Never use `padding-left: 84px`: that moves the brand
to the right of the traffic lights. `?mock=1` previews have no traffic lights,
so verify the inset in the built app.

## Releases and copy

- Version lives in `Cargo.toml` (workspace), `app/package.json`,
  `app/src-tauri/tauri.conf.json`, `app/src/App.tsx` and the Settings about
  line. Use `scripts/bump-version.sh x.y.z "note"`; keep CHANGELOG newest
  header at the top, no Unreleased section.
- Public-facing copy: no em dashes, no en dashes, ASCII hyphen only.
- Icons: `assets/logo-source.png` is the mark. Regenerate with
  `npx --prefix app tauri icon assets/logo-source.png` and copy the same file
  to `app/src/assets/logo.png`.

## Demo videos and guides

GODsend apps carry per-feature demo reels and a guides page, recorded from
the renderer's `?mock=1` bridge by `~/Code/demo-reels`. A new, changed or
removed feature updates its reel and its guides entry in the same change.
The PS5 app's reels are not recorded yet.
