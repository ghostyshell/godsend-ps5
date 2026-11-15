// Types shared with the Rust core (keep in sync with core/src serializers).

export interface Host {
  id: string;
  name: string;
  addr: string;
  ftp_port: number;
  ftp_user: string;
  ftp_pass: string;
  pad_port: number;
  payload_port: number;
  pkg_manager_port: number;
}

export interface Config {
  hosts: Host[];
  active_host: string | null;
  download_dir: string;
  rd_api_key: string;
  tb_api_key: string;
  gamesearch_base: string;
  covers_base: string;
  aria2_path: string;
  split_bytes: number;
  pkg_dir: string;
  homebrew_dir: string;
  auto_install_pkg: boolean;
  payload_catalog_url: string;
}

export interface DeviceStatus {
  host_id: string;
  name: string;
  addr: string;
  ftp_ok: boolean;
  ftp_ms: number;
  ftp_port: number | null;
  etahen_ok: boolean;
  etahen_version: string | null;
  homebrew_ok: boolean;
  pad_ok: boolean;
  payload_ok: boolean;
  pkg_manager_ok: boolean;
  pkg_manager_ms: number;
  message: string;
}

export interface PadStatus {
  connected: boolean;
  addr: string;
  buttons: number;
  lx: number;
  ly: number;
  rx: number;
  ry: number;
  l2: number;
  r2: number;
}

export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

export type JobState = "running" | "done" | "failed" | "cancelled";

export interface Job {
  id: number;
  kind: string;
  label: string;
  state: JobState;
  done_bytes: number;
  total_bytes: number;
  message: string;
  started_at: number;
  finished_at: number | null;
}

export interface Snapshot {
  hosts: Host[];
  active: string | null;
  config: Config;
  doctor: Check[];
  statuses: DeviceStatus[];
  jobs: Job[];
}

export interface GameEntry {
  id: string;
  title: string;
  title_id: string | null;
  system: string;
  kind: string;
  category: string;
  path: string;
  size_bytes: number;
  has_icon: boolean;
  icon_path: string | null;
  version: string | null;
  device: string;
}

export interface LibraryView {
  games: GameEntry[];
  packages: GameEntry[];
  roots: string[];
}

export interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  category: string;
  filename: string;
  url: string;
  source: string;
  version: string;
  published_at: string;
  size_bytes: number;
  checksum: string;
}

export interface GameLink {
  id: string;
  host: string;
  url: string;
  linkType: string;
  supportedBy: string[];
}

export interface GameHit {
  id: string;
  slug: string;
  title: string;
  platform: string;
  source: string;
  linkCount: number;
  rawgCover: string | null;
  cover: string | null;
  rating: number | null;
  released: string | null;
  links?: GameLink[];
}

export interface IaItem {
  id: string;
  title: string;
  files_count: number;
  size_bytes: number;
}

export interface IaFile {
  name: string;
  size_bytes: number;
  url: string;
}

export interface FmEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size_bytes: number;
  kind: string;
}

export interface ConvertInfo {
  path: string;
  name: string;
  size_bytes: number;
  kind: string;
  system: string;
  title_id: string | null;
  parts: number;
}

export const PAGE_TITLES: Record<string, string> = {
  home: "Console",
  devices: "Devices",
  library: "Library",
  controller: "Controller",
  store: "Game Index",
  downloads: "Downloads",
  files: "Files",
  convert: "Convert",
  payloads: "Payloads",
  settings: "Settings",
};

export const BUTTONS: { id: string; label: string }[] = [
  { id: "triangle", label: "△" },
  { id: "circle", label: "○" },
  { id: "cross", label: "✕" },
  { id: "square", label: "□" },
  { id: "up", label: "Up" },
  { id: "down", label: "Down" },
  { id: "left", label: "Left" },
  { id: "right", label: "Right" },
  { id: "l1", label: "L1" },
  { id: "l2", label: "L2" },
  { id: "l3", label: "L3" },
  { id: "r1", label: "R1" },
  { id: "r2", label: "R2" },
  { id: "r3", label: "R3" },
  { id: "create", label: "Create" },
  { id: "options", label: "Options" },
  { id: "ps", label: "PS" },
  { id: "touchpad", label: "Touchpad" },
];
