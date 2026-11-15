import { useMemo, useState } from "react";
import type { PageProps } from "../App";
import { api } from "../api";
import { Btn, Chip, Empty } from "../ui";

export default function Home({ snap, toast, reload, go }: PageProps) {
  const [busy, setBusy] = useState(false);
  const active = snap.statuses.find((s) => s.host_id === snap.active) ?? null;
  const host = snap.hosts.find((h) => h.id === snap.active) ?? null;
  const checks = useMemo(() => snap.doctor ?? [], [snap.doctor]);

  async function rescan() {
    setBusy(true);
    try {
      await api.probeAll();
      toast("probed every console");
      reload();
    } catch (err) {
      toast(`probe failed: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="page">
      <section className="hero">
        <div className="top">
          <div>
            <div className="eyebrow">CONSOLE</div>
            <h1>{host?.name ?? "No console selected"}</h1>
            <div className="sub">
              {active
                ? `${active.addr} - FTP ${active.ftp_ok ? `${active.ftp_ms} ms` : "offline"}${
                    active.etahen_ok ? ` - etaHEN ${active.etahen_version ?? "ready"}` : ""
                  }`
                : "Add a PS5 on the Devices page to begin."}
            </div>
          </div>
          <div style={{ display: "flex", gap: 8, alignItems: "flex-start", flexWrap: "wrap" }}>
            {active ? (
              <>
                <Chip tone={active.ftp_ok ? "good" : "bad"} live={active.ftp_ok}>
                  {active.ftp_ok ? "FTP online" : "FTP offline"}
                </Chip>
                <Chip tone={active.etahen_ok ? "good" : "warn"}>
                  {active.etahen_ok ? "etaHEN" : "no etaHEN"}
                </Chip>
                <Chip tone={active.pad_ok ? "good" : "warn"}>
                  {active.pad_ok ? "Ghostpad" : "no Ghostpad"}
                </Chip>
              </>
            ) : null}
          </div>
        </div>

        <div className="stats">
          <Stat k="ftp port" v={active?.ftp_port ? String(active.ftp_port) : "-"} />
          <Stat k="etahen" v={active?.etahen_version ?? "-"} />
          <Stat k="ghostpad" v={active?.pad_ok ? "ready" : "missing"} />
          <Stat k="elfldr" v={active?.payload_ok ? "ready" : "missing"} />
        </div>

        <div className="quick">
          <Btn variant="primary" disabled={!active?.ftp_ok} onClick={() => go("library")}>
            Open library
          </Btn>
          <Btn disabled={!active} onClick={() => go("controller")}>
            Virtual pad
          </Btn>
          <Btn disabled={!active} onClick={() => go("payloads")}>
            Payloads
          </Btn>
          <Btn disabled={busy} onClick={() => void rescan()}>
            {busy ? "Probing..." : "Probe consoles"}
          </Btn>
        </div>
      </section>

      <div className="panel">
        <div className="panel-head">
          <h2>Preflight</h2>
          <div className="spacer" />
          <Btn small disabled={busy} onClick={() => reload()}>
            Re-check
          </Btn>
        </div>
        <div className="checks">
          {checks.map((check) => (
            <div className="check" key={check.name}>
              <span className={`status-dot ${check.ok ? "ok" : "idle"}`} />
              <div>
                <div className="name">{check.name}</div>
                <div className="detail">{check.detail}</div>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel">
        <div className="panel-head">
          <h2>Consoles</h2>
          <div className="spacer" />
          <Btn small onClick={() => go("devices")}>
            Manage
          </Btn>
        </div>
        {snap.statuses.length === 0 ? (
          <Empty
            title="No consoles yet"
            hint="Add the PS5's LAN address. ftpsrv listens on 2121, etaHEN's FTP server on 1337."
            action={
              <Btn variant="primary" onClick={() => go("devices")}>
                Add a console
              </Btn>
            }
          />
        ) : (
          <div className="rows">
            {snap.statuses.map((status) => (
              <div className="row" key={status.host_id}>
                <span className={`status-dot ${status.ftp_ok ? "ok" : "off"}`} />
                <div>
                  <div className="title">{status.name}</div>
                  <div className="sub">
                    {status.addr} - {status.etahen_version ? `etaHEN ${status.etahen_version}` : "etaHEN not seen"}
                  </div>
                </div>
                <div className="grow" />
                <span className="size mono">{status.ftp_ok ? `${status.ftp_ms} ms` : "offline"}</span>
                {status.host_id !== snap.active ? (
                  <Btn
                    small
                    onClick={async () => {
                      await api.hostSetActive(status.host_id);
                      toast(`active console: ${status.name}`);
                      reload();
                    }}
                  >
                    Use
                  </Btn>
                ) : (
                  <Chip tone="laser">active</Chip>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="panel">
        <div className="panel-head">
          <h2>Install folders</h2>
          <div className="spacer" />
          <Btn small onClick={() => go("settings")}>
            Edit
          </Btn>
        </div>
        <div className="rows">
          <div className="row">
            <div>
              <div className="title">Game images and folder dumps</div>
              <div className="sub mono">{snap.config.homebrew_dir}</div>
            </div>
            <div className="grow" />
            <Chip tone={active?.homebrew_ok ? "good" : "warn"}>
              {active?.homebrew_ok ? "reachable" : "missing"}
            </Chip>
          </div>
          <div className="row">
            <div>
              <div className="title">Package queue</div>
              <div className="sub mono">{snap.config.pkg_dir}</div>
            </div>
            <div className="grow" />
            <Chip tone="neutral">PS5Upload reads it</Chip>
          </div>
        </div>
      </div>
    </div>
  );
}

function Stat({ k, v }: { k: string; v: string }) {
  return (
    <div className="stat">
      <div className="v">{v}</div>
      <div className="k">{k}</div>
    </div>
  );
}
