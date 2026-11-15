import { useCallback, useEffect, useMemo, useState } from "react";
import { api, listen } from "./api";
import type { Job, Snapshot } from "./types";
import { PAGE_TITLES } from "./types";
import { Chip } from "./ui";
import {
  House,
  MonitorSmartphone,
  Library as LibraryIcon,
  Gamepad2,
  Store as StoreIcon,
  Download as DownloadIcon,
  FolderOpen,
  ArrowLeftRight,
  Terminal,
  Settings as SettingsIcon,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import logo from "./assets/logo.png";
import Home from "./pages/Home";
import Devices from "./pages/Devices";
import Library from "./pages/Library";
import Controller from "./pages/Controller";
import GameStore from "./pages/GameStore";
import Downloads from "./pages/Downloads";
import Files from "./pages/Files";
import Convert from "./pages/Convert";
import Payloads from "./pages/Payloads";
import Settings from "./pages/Settings";

type Page =
  | "home"
  | "devices"
  | "library"
  | "controller"
  | "store"
  | "downloads"
  | "files"
  | "convert"
  | "payloads"
  | "settings";

const NAV: { id: Page; label: string; icon: LucideIcon }[] = [
  { id: "home", label: "Console", icon: House },
  { id: "devices", label: "Devices", icon: MonitorSmartphone },
  { id: "library", label: "Library", icon: LibraryIcon },
  { id: "controller", label: "Controller", icon: Gamepad2 },
  { id: "store", label: "Game Index", icon: StoreIcon },
  { id: "downloads", label: "Downloads", icon: DownloadIcon },
  { id: "files", label: "Files", icon: FolderOpen },
  { id: "convert", label: "Convert", icon: ArrowLeftRight },
  { id: "payloads", label: "Payloads", icon: Terminal },
  { id: "settings", label: "Settings", icon: SettingsIcon },
];

const VERSION = "0.1.0";

export interface PageProps {
  snap: Snapshot;
  reload: () => void;
  toast: (message: string) => void;
  refreshKey: number;
  go: (page: string) => void;
}

export default function App() {
  const [page, setPage] = useState<Page>("home");
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [toastMsg, setToastMsg] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  const toast = useCallback((message: string) => {
    setToastMsg(message);
    window.setTimeout(() => setToastMsg((cur) => (cur === message ? null : cur)), 4600);
  }, []);

  const loadSnapshot = useCallback(async () => {
    try {
      setSnap(await api.snapshot());
    } catch (err) {
      console.error("snapshot failed", err);
    }
  }, []);

  // Pages call reload() after any host/config change; without the snapshot
  // refresh those changes stay invisible until the app restarts.
  const reload = useCallback(() => {
    setRefreshKey((k) => k + 1);
    void loadSnapshot();
  }, [loadSnapshot]);

  useEffect(() => {
    void loadSnapshot();
    const unlisteners: Promise<() => void>[] = [
      listen<{ id: number; done: number; total: number }>("job://progress", (update) => {
        setSnap((cur) =>
          cur
            ? {
                ...cur,
                jobs: cur.jobs.map((job) =>
                  job.id === update.id
                    ? { ...job, done_bytes: update.done, total_bytes: update.total }
                    : job
                ),
              }
            : cur
        );
      }),
      listen<Job>("job://job", (job) => {
        setSnap((cur) => {
          if (!cur) return cur;
          const exists = cur.jobs.some((j) => j.id === job.id);
          return {
            ...cur,
            jobs: exists
              ? cur.jobs.map((j) => (j.id === job.id ? job : j))
              : [job, ...cur.jobs],
          };
        });
        if (job.state === "done") toast(`${job.label}: ${job.message || "done"}`);
        else if (job.state === "failed") toast(`${job.label} failed: ${job.message}`);
        setRefreshKey((k) => k + 1);
      }),
    ];
    return () => {
      void Promise.all(unlisteners).then((uns) => uns.forEach((fn) => fn()));
    };
  }, [loadSnapshot, toast]);

  const activeStatus = useMemo(() => {
    if (!snap?.active) return null;
    return snap.statuses.find((s) => s.host_id === snap.active) ?? null;
  }, [snap]);

  const activeHost = useMemo(
    () => snap?.hosts.find((h) => h.id === snap.active) ?? null,
    [snap]
  );

  if (!snap) {
    return (
      <div className="shell" aria-busy="true">
        <div className="backdrop">
          <div className="orb cyan" />
          <div className="orb indigo" />
          <div className="orb brass" />
        </div>
      </div>
    );
  }

  const pageProps: PageProps = { snap, reload, toast, refreshKey, go: (p) => setPage(p as Page) };

  return (
    <div className="shell">
      <div className="backdrop" aria-hidden="true">
        <div className="orb cyan" />
        <div className="orb indigo" />
        <div className="orb brass" />
      </div>

      <a className="skip-link" href="#workspace">
        Skip to content
      </a>

      <header className="topbar">
        <div className="brand">
          <img className="mark" src={logo} alt="" aria-hidden="true" />
          <div>
            <div className="eyebrow">GODSEND PS5</div>
            <div className="page-title" aria-live="polite">
              {PAGE_TITLES[page]}
            </div>
          </div>
        </div>
        <div className="spacer" />
        {activeStatus ? (
          <Chip
            tone={activeStatus.ftp_ok ? "good" : "warn"}
            title={`${activeStatus.addr} - FTP ${activeStatus.ftp_ok ? "up" : "down"}`}
          >
            {activeHost?.name ?? activeStatus.name}
          </Chip>
        ) : (
          <Chip tone="bad">No console</Chip>
        )}
      </header>

      <nav className="rail" aria-label="Sections">
        <div className="rail-caption">Console</div>
        {NAV.map((item) => (
          <button
            key={item.id}
            className={`rail-item ${page === item.id ? "on" : ""}`}
            aria-current={page === item.id ? "page" : undefined}
            onClick={() => setPage(item.id)}
            onKeyDown={(e) => {
              if (e.altKey && e.key >= "1" && e.key <= "9") {
                const idx = Number(e.key) - 1;
                if (NAV[idx]) setPage(NAV[idx].id);
              }
            }}
          >
            <item.icon className="glyph-icon" size={17} aria-hidden="true" />
            <span className="label">{item.label}</span>
          </button>
        ))}
      </nav>

      <main className="workspace with-inspector" id="workspace" tabIndex={-1}>
        {page === "home" && <Home {...pageProps} />}
        {page === "devices" && <Devices {...pageProps} />}
        {page === "library" && <Library {...pageProps} />}
        {page === "controller" && <Controller {...pageProps} />}
        {page === "store" && <GameStore {...pageProps} />}
        {page === "downloads" && <Downloads {...pageProps} />}
        {page === "files" && <Files {...pageProps} />}
        {page === "convert" && <Convert {...pageProps} />}
        {page === "payloads" && <Payloads {...pageProps} />}
        {page === "settings" && <Settings {...pageProps} />}
      </main>

      <aside className="inspector" aria-label="Session">
        <div className="session-card">
          <h3>Console</h3>
          {activeStatus ? (
            <>
              <div className="kv">
                <span className="k">host</span>
                <span className="v" title={activeStatus.addr}>
                  {activeStatus.addr}
                </span>
              </div>
              <div className="kv">
                <span className="k">ftp</span>
                <span className="v">
                  {activeStatus.ftp_ok
                    ? `${activeStatus.ftp_ms} ms${activeStatus.ftp_port ? ` :${activeStatus.ftp_port}` : ""}`
                    : "offline"}
                </span>
              </div>
              <div className="kv">
                <span className="k">etahen</span>
                <span className="v">
                  {activeStatus.etahen_ok
                    ? activeStatus.etahen_version ?? "running"
                    : "missing"}
                </span>
              </div>
              <div className="kv">
                <span className="k">ghostpad</span>
                <span className="v">{activeStatus.pad_ok ? "ready" : "missing"}</span>
              </div>
              <div className="kv">
                <span className="k">elfldr</span>
                <span className="v">{activeStatus.payload_ok ? "ready" : "missing"}</span>
              </div>
              <div className="kv">
                <span className="k">pkg mgr</span>
                <span className="v">
                  {activeStatus.pkg_manager_ok ? `${activeStatus.pkg_manager_ms} ms` : "-"}
                </span>
              </div>
            </>
          ) : (
            <p style={{ margin: 0, fontSize: 12.5, color: "var(--text-faint)" }}>
              Add the console's IP on the Devices page. ftpsrv (port 2121) or
              etaHEN's FTP server (port 1337) must be running on it.
            </p>
          )}
        </div>

        <div className="queue-card">
          <div className="panel-head" style={{ marginBottom: 0 }}>
            <h3 style={{ margin: 0 }}>Queue</h3>
            <div className="spacer" />
            {snap.jobs.some((j) => j.state !== "running") ? (
              <button
                className="btn small"
                onClick={async () => {
                  await api.jobsClear();
                  void loadSnapshot();
                }}
              >
                Clear
              </button>
            ) : null}
          </div>
          <div className="rows">
            {snap.jobs.length === 0 ? (
              <p style={{ margin: 0, fontSize: 12.5, color: "var(--text-faint)" }}>
                No jobs yet. Transfers, installs and conversions show up here.
              </p>
            ) : (
              snap.jobs.slice(0, 8).map((job) => (
                <JobRow
                  key={job.id}
                  job={job}
                  onCancel={async () => {
                    await api.jobCancel(job.id);
                    void loadSnapshot();
                  }}
                />
              ))
            )}
          </div>
        </div>
      </aside>

      <footer className="statusbar">
        <span>
          <span className={`status-dot ${activeStatus?.ftp_ok ? "ok" : "off"}`} />
          {activeHost ? `${activeHost.name} (${activeHost.addr})` : "no console"}
        </span>
        <span>
          <span className={`status-dot ${activeStatus?.etahen_ok ? "ok" : "idle"}`} />
          etaHEN {activeStatus?.etahen_ok ? (activeStatus.etahen_version ?? "ready") : "missing"}
        </span>
        <span>
          <span className={`status-dot ${activeStatus?.pad_ok ? "ok" : "idle"}`} />
          Ghostpad {activeStatus?.pad_ok ? "ready" : "missing"}
        </span>
        <span className="grow" />
        {activeStatus ? (
          <span className="mono">
            {activeStatus.homebrew_ok ? "homebrew folder ready" : "no homebrew folder"}
          </span>
        ) : null}
        <span>godsend-ps5 v{VERSION}</span>
      </footer>

      {toastMsg ? (
        <div className="toast" role="status" aria-live="polite">
          <span className="chip laser">
            <span className="dot" />
            PS5
          </span>
          {toastMsg}
        </div>
      ) : null}
    </div>
  );
}

function JobRow({ job, onCancel }: { job: Job; onCancel: () => void }) {
  const running = job.state === "running";
  return (
    <div className="job-row">
      <div className="head">
        <span className="label" title={job.label}>
          {job.label}
        </span>
        {running ? (
          <button className="btn small" onClick={onCancel} title="Cancel job">
            Cancel
          </button>
        ) : (
          <Chip tone={job.state === "done" ? "good" : job.state === "failed" ? "bad" : "warn"}>
            {job.state}
          </Chip>
        )}
      </div>
      {running ? (
        <div style={{ marginTop: 6 }}>
          <div className="bar live">
            <div
              className="fill"
              style={{
                width: `${
                  job.total_bytes ? Math.round((job.done_bytes / job.total_bytes) * 100) : 12
                }%`,
              }}
            />
          </div>
        </div>
      ) : (
        <div className="msg">{job.message}</div>
      )}
    </div>
  );
}
