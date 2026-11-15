import { useEffect, useState } from "react";
import type { PageProps } from "../App";
import { api } from "../api";
import type { GameHit, IaFile, IaItem } from "../types";
import { Btn, Empty, Seg, fmtBytes } from "../ui";

type Source = "gamesearch" | "ia";

export default function GameStore({ toast, reload, go }: PageProps) {
  const [source, setSource] = useState<Source>("gamesearch");
  const [query, setQuery] = useState("God of War");
  const [games, setGames] = useState<GameHit[]>([]);
  const [items, setItems] = useState<IaItem[]>([]);
  const [files, setFiles] = useState<Record<string, IaFile[]>>({});
  const [busy, setBusy] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  async function search(nextQuery = query, nextSource = source) {
    setBusy(true);
    setHint(null);
    try {
      const result = await api.storeSearch(nextQuery, nextSource);
      if (nextSource === "ia") {
        setItems(result.items ?? []);
        setGames([]);
      } else {
        setGames(result.games ?? []);
        setItems([]);
      }
      if ((result.games ?? []).length === 0 && (result.items ?? []).length === 0) {
        setHint("nothing matched; try a shorter query");
      }
    } catch (err) {
      setHint(String(err));
      setGames([]);
      setItems([]);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void search();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source]);

  async function downloadGame(hit: GameHit) {
    try {
      await api.storeDownload("gamesearch", hit.slug);
      toast(`downloading ${hit.title}`);
      reload();
    } catch (err) {
      toast(String(err));
    }
  }

  async function loadFiles(item: IaItem) {
    try {
      setFiles((cur) => ({ ...cur, [item.id]: [] }));
      const list = await api.storeFiles(item.id);
      setFiles((cur) => ({ ...cur, [item.id]: list }));
    } catch (err) {
      toast(`could not list ${item.title}: ${String(err)}`);
    }
  }

  return (
    <div className="page">
      <div className="panel">
        <div className="panel-head">
          <h2>Game index</h2>
          <div className="spacer" />
          <Seg<Source>
            ariaLabel="Store source"
            value={source}
            onChange={setSource}
            options={[
              { value: "gamesearch", label: "Game index" },
              { value: "ia", label: "Internet Archive" },
            ]}
          />
        </div>
        <div className="quick">
          <input
            type="text"
            className="search"
            placeholder={source === "ia" ? "PS5 game, dump, homebrew" : "Game title"}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void search();
            }}
          />
          <Btn variant="primary" disabled={busy} onClick={() => void search()}>
            {busy ? "Searching..." : "Search"}
          </Btn>
        </div>
        <p style={{ marginBottom: 0, fontSize: 12.5, color: "var(--text-faint)" }}>
          {source === "gamesearch"
            ? "The game index needs the Go service URL in Settings to resolve download links; browsing works against the public site."
            : "Internet Archive downloads are direct and need no debrid account."}
        </p>
      </div>

      {hint ? <Empty title="Nothing found" hint={hint} /> : null}

      {source === "gamesearch" && games.length > 0 ? (
        <div className="rows">
          {games.map((hit) => (
            <div className="row" key={hit.slug}>
              {hit.rawgCover || hit.cover ? (
                <img
                  src={hit.rawgCover ?? hit.cover ?? ""}
                  alt=""
                  style={{ width: 40, height: 54, objectFit: "cover", borderRadius: 6 }}
                />
              ) : null}
              <div>
                <div className="title">{hit.title}</div>
                <div className="sub">
                  {hit.platform.toUpperCase()} - {hit.source} - {hit.linkCount} link
                  {hit.linkCount === 1 ? "" : "s"}
                  {hit.released ? ` - ${hit.released}` : ""}
                </div>
              </div>
              <div className="grow" />
              <Btn small variant="primary" onClick={() => void downloadGame(hit)}>
                Download
              </Btn>
            </div>
          ))}
        </div>
      ) : null}

      {source === "ia" && items.length > 0 ? (
        <div className="rows">
          {items.map((item) => (
            <div key={item.id}>
              <div className="row">
                <div>
                  <div className="title">{item.title}</div>
                  <div className="sub mono">
                    {item.id} - {item.files_count} files - {fmtBytes(item.size_bytes)}
                  </div>
                </div>
                <div className="grow" />
                <Btn small onClick={() => void loadFiles(item)}>
                  Show files
                </Btn>
              </div>
              {files[item.id] ? (
                <div style={{ paddingLeft: 24 }}>
                  {files[item.id].length === 0 ? (
                    <p style={{ fontSize: 12.5, color: "var(--text-faint)" }}>No ISO, PKG or ZIP files in this item.</p>
                  ) : (
                    files[item.id].map((file) => (
                      <div className="row" key={file.name}>
                        <div>
                          <div className="title mono">{file.name}</div>
                          <div className="sub">{fmtBytes(file.size_bytes)}</div>
                        </div>
                        <div className="grow" />
                        <Btn
                          small
                          variant="primary"
                          onClick={async () => {
                            await api.storeDownload("ia", item.id, file.name);
                            toast(`downloading ${file.name}`);
                            reload();
                          }}
                        >
                          Download
                        </Btn>
                      </div>
                    ))
                  )}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}

      <div className="panel">
        <div className="panel-head">
          <h2>After the download</h2>
          <div className="spacer" />
          <Btn small onClick={() => go("downloads")}>
            Open downloads
          </Btn>
        </div>
        <p style={{ margin: 0, color: "var(--text-dim)", fontSize: 13 }}>
          Packages, images and ZIPs land in the downloads folder. Send them to
          the console from the Downloads page; a PKG goes to the package queue
          that PS5Upload or PKG Manager installs from, an image goes to the
          homebrew folder that ShadowMount+ mounts.
        </p>
      </div>
    </div>
  );
}
