// Typed bridge to the Tauri runtime. In ?mock=1 mode mock.ts installs
// window.__TAURI_INTERNALS__ so this file works unchanged in a browser.

import type {
  CatalogEntry,
  Check,
  Config,
  ConvertInfo,
  DeviceStatus,
  FmEntry,
  GameEntry,
  GameHit,
  Host,
  IaFile,
  IaItem,
  Job,
  LibraryView,
  PadStatus,
  Snapshot,
} from "./types";

export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const internals = (window as unknown as Record<string, any>).__TAURI_INTERNALS__;
  return internals.invoke(cmd, args);
}

type UnlistenFn = () => void;

export function listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  const api = (window as unknown as Record<string, any>).__TAURI_INTERNALS__;
  const wrapped = (e: { payload: T }) => handler(e.payload);
  return api
    .invoke("plugin:event|listen", {
      event,
      handler: api.transformCallback(wrapped),
      target: { kind: "Any" },
    })
    .then((id: number) => () => {
      void api.invoke("plugin:event|unlisten", { event, eventId: id });
    });
}

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  probeHost: (id?: string) => invoke<DeviceStatus>("probe_host", { id: id ?? null }),
  probeAll: () => invoke<DeviceStatus[]>("probe_all"),
  discover: (subnet: string) => invoke<Host[]>("discover", { subnet }),

  hostAdd: (host: Host, replaceId?: string | null) =>
    invoke<Config>("host_add", { host, replaceId: replaceId ?? null }),
  hostRemove: (id: string) => invoke<Config>("host_remove", { id }),
  hostSetActive: (id: string) => invoke<Config>("host_set_active", { id }),

  libraryScan: () => invoke<LibraryView>("library_scan"),
  icon: (path: string) => invoke<string | null>("icon", { path }),
  libraryDelete: (path: string, isDir: boolean) =>
    invoke<void>("library_delete", { path, isDir }),

  padConnect: () => invoke<PadStatus>("pad_connect"),
  padStatus: () => invoke<PadStatus>("pad_status"),
  padDisconnect: () => invoke<void>("pad_disconnect"),
  padPress: (button: string) => invoke<void>("pad_press", { button }),
  padHold: (button: string) => invoke<void>("pad_hold", { button }),
  padRelease: (button?: string) => invoke<void>("pad_release", { button: button ?? null }),
  padCombo: (buttons: string[], holdMs: number) =>
    invoke<void>("pad_combo", { buttons, holdMs }),
  padAnalog: (side: string, direction: string) =>
    invoke<void>("pad_analog", { side, direction }),
  padMacro: (script: string) => invoke<void>("pad_macro", { script }),

  payloadCatalog: (refresh: boolean) =>
    invoke<CatalogEntry[]>("payload_catalog", { refresh }),
  payloadInstall: (id: string) => invoke<number>("payload_install", { id }),
  payloadSend: (path: string) => invoke<number>("payload_send", { path }),

  storeSearch: (query: string, source: "gamesearch" | "ia") =>
    invoke<{ kind: string; games?: GameHit[]; items?: IaItem[] }>("store_search", {
      query,
      source,
    }),
  storeFiles: (item: string) => invoke<IaFile[]>("store_files", { item }),
  storeDownload: (source: string, id: string, name?: string) =>
    invoke<number>("store_download", { source, id, name: name ?? null }),

  downloadsList: () =>
    invoke<{ dir: string; files: FmEntry[] }>("downloads_list"),
  downloadDelete: (name: string) => invoke<void>("download_delete", { name }),
  downloadOpen: (name: string) => invoke<string>("download_open", { name }),
  downloadPush: (name: string) => invoke<number>("download_push", { name }),
  installPath: (path: string) => invoke<number>("install_path", { path }),

  fmLocal: (path: string) => invoke<FmEntry[]>("fm_local", { path }),
  fmRemote: (path: string) => invoke<FmEntry[]>("fm_remote", { path }),
  fmMkdir: (path: string, name: string) => invoke<string>("fm_mkdir", { path, name }),
  fmMkdirRemote: (path: string) => invoke<void>("fm_mkdir_remote", { path }),
  fmDelete: (path: string, isDir: boolean, remote: boolean) =>
    invoke<void>("fm_delete", { path, isDir, remote }),
  fmUpload: (local: string, remoteDir: string) =>
    invoke<number>("fm_upload", { local, remoteDir }),
  fmDownload: (remote: string, localDir: string) =>
    invoke<number>("fm_download", { remote, localDir }),

  convertInspect: (path: string) => invoke<ConvertInfo>("convert_inspect", { path }),
  convertRun: (kind: string, src: string, out: string, chunkMb?: number) =>
    invoke<number>("convert_run", {
      kind,
      src,
      out,
      chunkMb: chunkMb ?? null,
    }),

  configGet: () => invoke<Config>("config_get"),
  configSet: (patch: Partial<Config>) => invoke<Config>("config_set", { patch }),
  doctorRun: () => invoke<Check[]>("doctor_run"),

  jobs: () => invoke<Job[]>("jobs"),
  jobCancel: (id: number) => invoke<boolean>("job_cancel", { id }),
  jobsClear: () => invoke<void>("jobs_clear"),

  reveal: (path: string) => invoke<void>("reveal", { path }),
  openDataDir: () => invoke<string>("open_data_dir"),
};

export const pickers = {
  async pickFiles(title: string, extensions: string[]): Promise<string[]> {
    const api = (window as unknown as Record<string, any>).__TAURI_INTERNALS__;
    const result = (await api.invoke("plugin:dialog|open", {
      options: { title, multiple: true, filters: [{ name: "Files", extensions }] },
    })) as string | string[] | null;
    if (!result) return [];
    return Array.isArray(result) ? result : [result];
  },
  async pickFile(title: string, extensions: string[]): Promise<string | null> {
    const files = await pickers.pickFiles(title, extensions);
    return files.length ? files[0] : null;
  },
  async pickDirectory(title: string): Promise<string | null> {
    const api = (window as unknown as Record<string, any>).__TAURI_INTERNALS__;
    const result = (await api.invoke("plugin:dialog|open", {
      options: { title, directory: true, multiple: false },
    })) as string | null;
    return result ?? null;
  },
  async pickSavePath(title: string, defaultName: string, extensions: string[]): Promise<string | null> {
    const api = (window as unknown as Record<string, any>).__TAURI_INTERNALS__;
    const result = (await api.invoke("plugin:dialog|save", {
      options: { title, defaultPath: defaultName, filters: [{ name: "File", extensions }] },
    })) as string | null;
    return result ?? null;
  },
};

export type { CatalogEntry, Config, DeviceStatus, GameEntry, GameHit, Host, Job, PadStatus };
