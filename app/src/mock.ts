// Mock bridge: installs window.__TAURI_INTERNALS__ with PS5 fixture data so
// the UI runs in a plain browser (?mock=1). Never imported by the app.

type Handler = (payload: unknown) => void;

const listeners = new Map<number, { event: string; handler: Handler }>();
let nextCallbackId = 1;

const GB = 1024 * 1024 * 1024;
const MB = 1024 * 1024;

const host = {
  id: "192.168.1.50:2121",
  name: "Living room PS5",
  addr: "192.168.1.50",
  ftp_port: 2121,
  ftp_user: "anonymous",
  ftp_pass: "",
  pad_port: 6967,
  payload_port: 9021,
  pkg_manager_port: 8844,
};

let config = {
  hosts: [host],
  active_host: host.id,
  download_dir: "/Users/demo/Downloads/godsend-ps5",
  rd_api_key: "",
  tb_api_key: "",
  gamesearch_base: "https://game-debrid.link/api",
  covers_base: "https://covers.debrid.in",
  aria2_path: "",
  split_bytes: 4095 * MB,
  pkg_dir: "/data/ps5upload/pkg_library",
  homebrew_dir: "/data/homebrew",
  auto_install_pkg: true,
  payload_catalog_url:
    "https://raw.githubusercontent.com/itsPLK/ps5-payloads-mirror/main/payloads_v2.json",
};

const status = {
  host_id: host.id,
  name: host.name,
  addr: host.addr,
  ftp_ok: true,
  ftp_ms: 12,
  ftp_port: 2121,
  etahen_ok: true,
  etahen_version: "2.5B",
  homebrew_ok: true,
  pad_ok: true,
  payload_ok: true,
  pkg_manager_ok: true,
  pkg_manager_ms: 21,
  message: "# etaHEN config",
};

const games = [
  {
    id: "app:/user/app/PPSA01234",
    title: "Demon's Souls",
    title_id: "PPSA01234",
    system: "ps5",
    kind: "app",
    category: "digital",
    path: "/user/app/PPSA01234",
    size_bytes: 52 * GB,
    has_icon: true,
    icon_path: "/user/appmeta/PPSA01234/icon0.png",
    version: "01.002.000",
    device: "internal",
  },
  {
    id: "app:/user/app/PPSA07777",
    title: "Astro's Playroom",
    title_id: "PPSA07777",
    system: "ps5",
    kind: "app",
    category: "digital",
    path: "/user/app/PPSA07777",
    size_bytes: 10.4 * GB,
    has_icon: true,
    icon_path: "/user/appmeta/PPSA07777/icon0.png",
    version: "01.000.000",
    device: "internal",
  },
  {
    id: "app:/user/app/CUSA12345",
    title: "The Last of Us Part II",
    title_id: "CUSA12345",
    system: "ps4",
    kind: "app",
    category: "digital",
    path: "/user/app/CUSA12345",
    size_bytes: 78 * GB,
    has_icon: true,
    icon_path: "/user/appmeta/CUSA12345/icon0.png",
    version: "01.09",
    device: "internal",
  },
  {
    id: "image:/data/homebrew/PPSA05555/God of War Ragnarok.exfat",
    title: "God of War Ragnarok",
    title_id: "PPSA05555",
    system: "homebrew",
    kind: "image",
    category: "homebrew",
    path: "/data/homebrew/PPSA05555/God of War Ragnarok.exfat",
    size_bytes: 84 * GB,
    has_icon: true,
    icon_path: "/data/homebrew/PPSA05555/sce_sys/icon0.png",
    version: null,
    device: "internal",
  },
  {
    id: "image:/data/homebrew/Elden Ring/Elden Ring.ffpkg",
    title: "Elden Ring",
    title_id: null,
    system: "homebrew",
    kind: "image",
    category: "homebrew",
    path: "/data/homebrew/Elden Ring/Elden Ring.ffpkg",
    size_bytes: 48 * GB,
    has_icon: false,
    icon_path: null,
    version: null,
    device: "internal",
  },
  {
    id: "image:/mnt/usb0/homebrew/RetroArch/RetroArch.ffpfsc",
    title: "RetroArch",
    title_id: null,
    system: "homebrew",
    kind: "image",
    category: "homebrew",
    path: "/mnt/usb0/homebrew/RetroArch/RetroArch.ffpfsc",
    size_bytes: 2.1 * GB,
    has_icon: false,
    icon_path: null,
    version: null,
    device: "usb0",
  },
];

const packages = [
  {
    id: "pkg:/data/ps5upload/pkg_library/base.pkg",
    title: "PPSA09999",
    title_id: "PPSA09999",
    system: "pkg",
    kind: "pkg",
    category: "package",
    path: "/data/ps5upload/pkg_library/base.pkg",
    size_bytes: 36 * GB,
    has_icon: false,
    icon_path: null,
    version: null,
    device: "internal",
  },
];

const payloads = [
  ["ghostpad_v1.2.elf", "Ghostpad", "Networking & Servers", "Virtual controller. This app's Controller page talks to it on 6967.", "v1.2", "2026-11-12"],
  ["elfldr_v0.27.elf", "elfldr", "Networking & Servers", "The payload loader every other payload needs. Listens on 9021.", "v0.27", "2026-11-09"],
  ["ftpsrv_v0.21.1.elf", "ftpsrv", "Networking & Servers", "Standalone FTP server on port 2121.", "v0.21.1", "2026-11-08"],
  ["ShadowMountPlus_v1.7beta11.elf", "ShadowMountPlus", "Utilities & Tools", "Mounts game images dropped into /data/homebrew.", "1.7beta11", "2026-11-10"],
  ["kstuff-lite_v1.11.elf", "kstuff-lite", "Kernel", "Kernel patches needed by fake package installs.", "v1.11", "2026-11-05"],
  ["etaHEN_2.5B.elf", "etaHEN", "Networking & Servers", "The all-in-one homebrew enabler: FTP, PKG Manager, payload menu.", "2.5B", "2026-11-11"],
  ["ps5upload_v12.4.20.elf", "ps5upload", "Networking & Servers", "Install helper used by the PS5Upload desktop app.", "v12.4.20", "2026-11-14"],
  ["pldmgr_v0.6.3.elf", "Payload Manager", "Utilities & Tools", "Browses, imports and auto-loads payloads on the console.", "v0.6.3", "2026-11-13"],
].map(([filename, name, category, description, version, published_at]) => ({
  id: filename,
  name,
  description,
  category,
  filename,
  url: `https://github.com/itsPLK/ps5-payloads-mirror/releases/download/payloads-mirror/${filename}`,
  source: "https://github.com/itsPLK/ps5-payloads-mirror",
  version,
  published_at,
  size_bytes: 3 * MB,
  checksum: "",
}));

const searchGames = [
  { id: "demon-s-ouls", slug: "demon-s-ouls", title: "Demon's Souls", platform: "ps5", source: "minerva", linkCount: 2, rawgCover: null, cover: null, rating: 4.5, released: "2020-11-12" },
  { id: "god-of-war-ragnarok", slug: "god-of-war-ragnarok", title: "God of War Ragnarok", platform: "ps5", source: "minerva", linkCount: 1, rawgCover: null, cover: null, rating: 4.8, released: "2022-11-09" },
  { id: "elden-ring", slug: "elden-ring", title: "Elden Ring", platform: "ps5", source: "minerva", linkCount: 1, rawgCover: null, cover: null, rating: 4.7, released: "2022-02-25" },
];

const iaItems = [
  { id: "ps5-dumps", title: "PlayStation 5 dumps", files_count: 12, size_bytes: 620 * GB },
  { id: "ps5-homebrew-archive", title: "PS5 homebrew archive", files_count: 18, size_bytes: 4.1 * GB },
];

const iaFiles = [
  { name: "God of War Ragnarok (USA).exfat", size_bytes: 84 * GB, url: "https://archive.org/x/gowr.exfat" },
  { name: "Demon's Souls (USA).ffpkg", size_bytes: 52 * GB, url: "https://archive.org/x/demons.ffpkg" },
];

const downloadFiles = [
  { name: "God of War Ragnarok.exfat", path: "/Users/demo/Downloads/godsend-ps5/God of War Ragnarok.exfat", is_dir: false, size_bytes: 84 * GB, kind: "image" },
  { name: "base.pkg", path: "/Users/demo/Downloads/godsend-ps5/base.pkg", is_dir: false, size_bytes: 36 * GB, kind: "pkg" },
  { name: "Ghostpad.elf", path: "/Users/demo/Downloads/godsend-ps5/Ghostpad.elf", is_dir: false, size_bytes: 3 * MB, kind: "elf" },
];

const remoteFiles = [
  { name: "app", path: "/user/app", is_dir: true, size_bytes: 0, kind: "folder" },
  { name: "appmeta", path: "/user/appmeta", is_dir: true, size_bytes: 0, kind: "folder" },
  { name: "homebrew", path: "/data/homebrew", is_dir: true, size_bytes: 0, kind: "folder" },
  { name: "pkg_library", path: "/data/ps5upload/pkg_library", is_dir: true, size_bytes: 0, kind: "folder" },
  { name: "base.pkg", path: "/data/ps5upload/pkg_library/base.pkg", is_dir: false, size_bytes: 36 * GB, kind: "pkg" },
];

let jobs = [
  {
    id: 2,
    kind: "payload",
    label: "Ghostpad",
    state: "done",
    done_bytes: 100,
    total_bytes: 100,
    message: "payload sent; the console logs the result in its kernel log",
    started_at: Math.floor(Date.now() / 1000) - 600,
    finished_at: Math.floor(Date.now() / 1000) - 590,
  },
  {
    id: 1,
    kind: "install",
    label: "God of War Ragnarok",
    state: "done",
    done_bytes: 100,
    total_bytes: 100,
    message: "uploaded to /data/homebrew/PPSA05555; ShadowMount+ mounts it within 15 seconds",
    started_at: Math.floor(Date.now() / 1000) - 3600,
    finished_at: Math.floor(Date.now() / 1000) - 3590,
  },
];

const doctor = [
  { name: "Console configured", ok: true, detail: "Living room PS5 at 192.168.1.50" },
  { name: "FTP reachable", ok: true, detail: "12 ms on 2121" },
  { name: "etaHEN", ok: true, detail: "v2.5B" },
  { name: "Ghostpad", ok: true, detail: "answering on 6967" },
  { name: "elfldr", ok: true, detail: "answering on 9021" },
  { name: "PKG Manager", ok: true, detail: "http://192.168.1.50:8844" },
  { name: "Download folder", ok: true, detail: "/Users/demo/Downloads/godsend-ps5" },
  { name: "aria2c", ok: true, detail: "/opt/homebrew/bin/aria2c" },
  { name: "Debrid", ok: false, detail: "optional: keys unlock debrid download links" },
];

const padStatus = {
  connected: false,
  addr: "",
  buttons: 0,
  lx: 128,
  ly: 128,
  rx: 128,
  ry: 128,
  l2: 0,
  r2: 0,
};

async function invoke(cmd: string, args: Record<string, any> = {}): Promise<unknown> {
  switch (cmd) {
    case "snapshot":
      return { hosts: config.hosts, active: config.active_host, config, doctor, statuses: [status], jobs };
    case "probe_host":
    case "probe_all":
      return cmd === "probe_all" ? [status] : status;
    case "discover":
      return [
        { id: "192.168.1.51:2121", name: "PS5 192.168.1.51", addr: "192.168.1.51", ftp_port: 2121, ftp_user: "anonymous", ftp_pass: "", pad_port: 6967, payload_port: 9021, pkg_manager_port: 8844 },
      ];
    case "host_add": {
      const incoming = args.host as any;
      config = { ...config, hosts: [...config.hosts.filter((h) => h.id !== incoming.id), incoming] };
      return config;
    }
    case "host_remove":
      config = { ...config, hosts: config.hosts.filter((h) => h.id !== args.id) };
      return config;
    case "host_set_active":
      config = { ...config, active_host: args.id };
      return config;
    case "library_scan":
      return {
        games,
        packages,
        roots: ["/user/app", "/user/appmeta", "/data/homebrew", "/data/ps5upload/pkg_library"],
      };
    case "icon":
      return iconDataUrl(String(args.path));
    case "pad_connect":
      padStatus.connected = true;
      padStatus.addr = `${host.addr}:${host.pad_port}`;
      return padStatus;
    case "pad_status":
      return padStatus;
    case "pad_disconnect":
      padStatus.connected = false;
      return null;
    case "pad_press":
    case "pad_hold":
    case "pad_release":
    case "pad_combo":
    case "pad_analog":
    case "pad_macro":
      if (!padStatus.connected) throw new Error("Ghostpad is not running on the console");
      return null;
    case "library_delete":
    case "config_set":
      if (cmd === "config_set") config = { ...config, ...(args.patch as object) };
      return cmd === "config_set" ? config : null;
    case "payload_catalog":
      return payloads;
    case "payload_install":
    case "payload_send":
    case "store_download":
    case "download_push":
    case "install_path":
    case "fm_upload":
    case "fm_download":
    case "convert_run": {
      const id = Math.max(0, ...jobs.map((j) => j.id)) + 1;
      jobs = [
        {
          id,
          kind: cmd,
          label: String(args.id ?? args.name ?? args.path ?? args.kind ?? "job"),
          state: "done",
          done_bytes: 100,
          total_bytes: 100,
          message: "done (mock)",
          started_at: Math.floor(Date.now() / 1000),
          finished_at: Math.floor(Date.now() / 1000),
        },
        ...jobs,
      ];
      return id;
    }
    case "store_search":
      return args.source === "ia"
        ? { kind: "ia", items: iaItems }
        : { kind: "gamesearch", games: searchGames };
    case "store_files":
      return iaFiles;
    case "downloads_list":
      return { dir: config.download_dir, files: downloadFiles };
    case "download_open":
    case "download_delete":
      return null;
    case "fm_local":
      return downloadFiles;
    case "fm_remote":
      return remoteFiles;
    case "fm_mkdir":
    case "fm_mkdir_remote":
      return "/Users/demo/Downloads/godsend-ps5/new-folder";
    case "fm_delete":
      return null;
    case "convert_inspect": {
      const path = String(args.path);
      const name = path.split("/").pop() ?? path;
      const lower = name.toLowerCase();
      const kind = lower.endsWith(".iso")
        ? "iso"
        : lower.endsWith(".pkg")
        ? "pkg"
        : lower.endsWith(".zip")
        ? "zip"
        : /\.(exfat|ffpkg|ffpfs|ffpfsc)$/.test(lower)
        ? "image"
        : lower.endsWith(".elf") || lower.endsWith(".bin")
        ? "elf"
        : "folder";
      return {
        path,
        name,
        size_bytes: 84 * GB,
        kind,
        system: kind === "image" ? "homebrew" : "ps5",
        title_id: "PPSA05555",
        parts: 0,
      };
    }
    case "config_get":
      return config;
    case "doctor_run":
      return doctor;
    case "jobs":
      return jobs;
    case "job_cancel":
      return true;
    case "jobs_clear":
      jobs = jobs.filter((j) => j.state === "running");
      return null;
    case "reveal":
    case "open_data_dir":
      return null;
    case "plugin:dialog|open": {
      const options = (args.options ?? {}) as { directory?: boolean; filters?: { extensions: string[] }[] };
      if (options.directory) return "/Users/demo/Downloads/godsend-ps5";
      const ext = options.filters?.[0]?.extensions?.[0] ?? "exfat";
      return `/Users/demo/Downloads/godsend-ps5/God of War Ragnarok.${ext}`;
    }
    case "plugin:dialog|save":
      return "/Users/demo/Downloads/godsend-ps5/output.ffpkg";
    case "plugin:event|listen": {
      const id = nextCallbackId++;
      listeners.set(id, { event: args.event, handler: args.handler });
      return id;
    }
    case "plugin:event|unlisten":
      listeners.delete(args.eventId);
      return null;
    default:
      console.warn("[mock] unhandled invoke:", cmd, args);
      return null;
  }
}

function iconDataUrl(seed: string): string {
  const canvas = document.createElement("canvas");
  canvas.width = 288;
  canvas.height = 160;
  const ctx = canvas.getContext("2d")!;
  let hash = 0;
  for (const ch of seed) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  const hue = Math.abs(hash) % 360;
  const grad = ctx.createLinearGradient(0, 0, 288, 160);
  grad.addColorStop(0, `hsl(${hue} 58% 34%)`);
  grad.addColorStop(1, `hsl(${(hue + 45) % 360} 62% 16%)`);
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, 288, 160);
  const glow = ctx.createRadialGradient(230, 10, 10, 230, 10, 160);
  glow.addColorStop(0, "rgba(255,255,255,0.22)");
  glow.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, 288, 160);
  ctx.fillStyle = "rgba(255,255,255,0.95)";
  ctx.font = "700 22px 'Chakra Petch', sans-serif";
  ctx.textBaseline = "bottom";
  const noise = new Set(["sce_sys", "appmeta", "app", "user", "data", "homebrew", "usb0", "ext0"]);
  const parts = seed.split("/").filter(Boolean);
  const title =
    [...parts]
      .reverse()
      .find((part) => !noise.has(part.toLowerCase()) && !part.toLowerCase().endsWith(".png")) ?? "PS5";
  ctx.fillText(title.slice(0, 24), 16, 128);
  return canvas.toDataURL("image/png");
}

(window as unknown as Record<string, any>).__TAURI_INTERNALS__ = {
  invoke,
  transformCallback(cb: Handler) {
    const id = nextCallbackId++;
    listeners.set(id, { event: "__callback__", handler: cb });
    return id;
  },
  metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main" } },
  plugins: {},
};

console.info("[mock] GODsend PS5 fixture bridge installed");
export {}; // make this a module so dynamic import type-checks
