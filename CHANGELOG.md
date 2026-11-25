# Changelog

## [Unreleased]

### Changed

- Accessibility pass: search and filter inputs have accessible names, file rows are keyboard selectable, docs links show a focus ring and honour reduced motion.
- Downloads are hosted on Cloudflare R2 under versioned, immutable keys, so a
  link stays valid until the next version. The weekly GoFile/file.kiwi re-upload
  is gone: the scheduled job now only checks the live feed for a version bump,
  and each publish prunes R2 versions older than the previous one.

## [0.1.0] - 2026-11-15

First release.

### Added

- Multi-console management with FTP probing and port fallback (2121 ftpsrv,
  1337 etaHEN, 21), etaHEN version detection, and LAN discovery on 2121 and
  1337.
- Service probing for Ghostpad (6967), elfldr (9021) and the PKG Manager web
  UI, surfaced in the console inspector and preflight checks.
- Library scan for installed apps from `/user/app` plus `/user/appmeta`,
  game images and folder dumps in `/data/homebrew`, `/data/etaHEN/games` and
  USB homebrew folders, and packages in the PS5Upload package queue.
  PARAM.SFO titles, title ids, versions and cached icons.
- Virtual controller over the Ghostpad payload: on-screen DualSense, keyboard
  mapping, press-and-hold, combos, macros and stick directions.
- Payload catalogue fetched at runtime from the ps5-payloads mirror, sent to
  elfldr, plus local ELF and BIN sending.
- Downloads from the GameSearch index and Internet Archive with aria2c or
  plain HTTP, in a cancellable job queue.
- Install pipeline: PKG to the package queue, images and folder dumps to the
  ShadowMount+ homebrew folder, ELF to elfldr, ZIP unpacking and FAT32
  splitting on USB destinations.
- Conversions: folder to ISO (ISO9660 plus Joliet, multi-extent), ISO to
  folder, ZIP pack and unpack, split and merge, and PKG/image inspection.
- Two-pane local and console file manager with upload, download, folder
  creation and delete.
- Headless CLI covering hosts, scan, files, install, payloads, pad,
  conversions, search and doctor.

### Notes

- Package installs are handed to PS5Upload or the etaHEN PKG Manager; the app
  leaves the file where they watch and reports that, rather than claiming an
  install it cannot perform over FTP.
- ShadowMount+ image formats are inspected and transferred, not built.
