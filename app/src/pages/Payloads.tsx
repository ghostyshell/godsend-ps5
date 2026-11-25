import { useCallback, useEffect, useMemo, useState } from "react";
import type { PageProps } from "../App";
import { api, pickers } from "../api";
import type { CatalogEntry } from "../types";
import { Btn, Chip, Empty, Seg, fmtBytes } from "../ui";

type Group = "all" | "payload" | "pkg";

export default function Payloads({ snap, toast, reload, refreshKey }: PageProps) {
  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [group, setGroup] = useState<Group>("all");
  const [query, setQuery] = useState("");

  const load = useCallback(
    async (refresh: boolean) => {
      setLoading(true);
      try {
        setEntries(await api.payloadCatalog(refresh));
      } catch (err) {
        toast(`catalogue failed: ${String(err)}`);
      } finally {
        setLoading(false);
      }
    },
    [toast]
  );

  useEffect(() => {
    void load(false);
  }, [load, refreshKey]);

  const visible = useMemo(() => {
    return entries.filter((entry) => {
      if (group !== "all" && kindOf(entry) !== group) return false;
      if (query.trim() && !`${entry.name} ${entry.description}`.toLowerCase().includes(query.trim().toLowerCase())) {
        return false;
      }
      return true;
    });
  }, [entries, group, query]);

  async function sendLocal() {
    const file = await pickers.pickFile("Choose an ELF payload", ["elf", "bin", "pkg"]);
    if (!file) return;
    try {
      await api.payloadSend(file);
      toast(`sending ${file.split("/").pop()}`);
      reload();
    } catch (err) {
      toast(`send failed: ${String(err)}`);
    }
  }

  return (
    <div className="page">
      <div className="panel">
        <div className="panel-head">
          <h2>Payloads and homebrew</h2>
          <div className="spacer" />
          <input
            className="search"
            type="text"
            aria-label="Filter payloads"
            placeholder="Filter"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <Btn small onClick={() => void sendLocal()}>Send local file</Btn>
          <Btn small disabled={loading} onClick={() => void load(true)}>
            {loading ? "Refreshing..." : "Refresh list"}
          </Btn>
        </div>
        <p style={{ marginTop: 0, color: "var(--text-dim)", fontSize: 13 }}>
          The list comes from the ps5-payloads mirror. ELFs go straight to
          elfldr on port {snap.hosts.find((h) => h.id === snap.active)?.payload_port ?? 9021};
          packages land in {snap.config.pkg_dir}. Without elfldr, use{" "}
          <span className="mono">Send local file</span> only for packages.
        </p>
        <Seg<Group>
          ariaLabel="Payload filter"
          value={group}
          onChange={setGroup}
          options={[
            { value: "all", label: "All" },
            { value: "payload", label: "Payloads" },
            { value: "pkg", label: "Packages" },
          ]}
        />
      </div>

      {visible.length === 0 ? (
        <Empty
          title={loading ? "Loading the catalogue..." : "Nothing in the catalogue"}
          hint="Refresh to pull the current payload list; the mirror is fetched at runtime."
        />
      ) : (
        <div className="store-grid">
          {visible.map((entry) => (
            <article className="store-card" key={entry.id}>
              <div className="head">
                <div className="name">{entry.name}</div>
                <span className="tag">{entry.category}</span>
              </div>
              <div className="desc">{entry.description}</div>
              <div className="repo mono">{entry.filename}</div>
              <div className="foot">
                <span className="mono" style={{ fontSize: 11.5, color: "var(--text-faint)" }}>
                  {entry.version || "no version"}
                  {entry.published_at ? ` - ${entry.published_at}` : ""}
                  {entry.size_bytes ? ` - ${fmtBytes(entry.size_bytes)}` : ""}
                </span>
                <div className="spacer" />
                <Chip tone={kindOf(entry) === "payload" ? "laser" : "neutral"}>
                  {kindOf(entry) === "payload" ? "ELF" : "PKG"}
                </Chip>
                <Btn
                  small
                  variant="primary"
                  onClick={async () => {
                    try {
                      await api.payloadInstall(entry.id);
                      toast(`sending ${entry.name}`);
                      reload();
                    } catch (err) {
                      toast(`send failed: ${String(err)}`);
                    }
                  }}
                >
                  Send
                </Btn>
              </div>
            </article>
          ))}
        </div>
      )}
    </div>
  );
}

function kindOf(entry: CatalogEntry): "payload" | "pkg" {
  const name = entry.filename.toLowerCase();
  return name.endsWith(".pkg") ? "pkg" : "payload";
}
