# AI guidelines for godsend-ps5

Read `AGENTS.md` first for the architecture and invariants, and `DESIGN.md`
before touching any UI code.

## Stack

Rust workspace (`core` engine, `cli`, `app/src-tauri` Tauri 2 shell)
plus a React 18 + TypeScript + Vite frontend with one hand-rolled stylesheet.
No CSS framework; design tokens live on `:root` in `app/src/styles.css` and
raw hex in components is banned.

## Rules

1. Keep `core/` framework-agnostic: no tauri, no UI types, no stdio. The CLI
   and the desktop app are thin layers over the engine.
2. Frontend types in `app/src/types.ts` must stay in sync with the serde
   shapes in core; the mock bridge (`app/src/mock.ts`) must mirror real
   fixture shapes so `?mock=1` stays honest.
3. Long operations are jobs in `godsend_core::transfer::JobManager` with
   progress and cancellation; never block the UI thread with I/O.
4. Destructive console operations require explicit confirmation in the UI and
   must path-escape-guard anything joined from user input.
5. Copy follows the no-em-dashes rule. Every product change bumps the version
   and writes a `CHANGELOG.md` entry in the same commit:
   `scripts/bump-version.sh x.y.z "Fixed: ..."`. No Unreleased section.
6. Design: follow DESIGN.md tokens and motion rules; respect the
   reduced-motion kill switch; keep state chips dot plus word.
7. Icons come from `assets/logo-source.png`. Regenerate with
   `npx --prefix app tauri icon assets/logo-source.png`; keep
   `app/src/assets/logo.png` the same mark.
8. Tests: `cargo test --workspace` covers parsers, the ISO writer, ZIP
   round trips, Ghostpad frames and the fake-console end-to-end flow;
   `npm --prefix app run test:ui` covers every page through the mock bridge.
   Both must pass before a commit.

## PS5 domain notes

- Detection is network based: FTP on 2121 (ftpsrv) or 1337 (etaHEN), elfldr
  on 9021, Ghostpad on 6967, PKG Manager on 8844. There is no mounted volume.
- Remote paths are absolute: `/user/app`, `/user/appmeta`, `/data/homebrew`,
  `/data/etaHEN/games`, `/mnt/usb*/homebrew`. `/data` is the same partition as
  `/user/data`.
- Installed apps are folders under `/user/app` named by title id; their
  metadata and icon live under `/user/appmeta/<TITLEID>/param.sfo` and
  `icon0.png`. PS5 ids start PPSA, PS4 ids CUSA.
- ShadowMount+ mounts images dropped in `/data/homebrew` within 15 seconds.
  `.exfat`, `.ffpkg`, `.ffpfs` and `.ffpfsc` are the formats it accepts.
- Packages are not installed by this app: they are left in
  `/data/ps5upload/pkg_library` (configurable) for PS5Upload or PKG Manager.
  Never report such an upload as installed.
- Ghostpad frames are 16 bytes: `GPAD`, big-endian button mask, LX/LY/RX/RY,
  L2/R2, two reserved bytes. 128 is stick centre. One device at a time.
- FAT32 splits are `name.0`, `name.1` at 4095 MiB by default.
