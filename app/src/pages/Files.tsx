import { useCallback, useEffect, useState } from "react";
import type { PageProps } from "../App";
import { api, pickers } from "../api";
import type { FmEntry } from "../types";
import { Btn, Empty, confirmDialog, fmtBytes } from "../ui";

export default function Files({ snap, toast, reload }: PageProps) {
  const [localPath, setLocalPath] = useState(snap.config.download_dir || "");
  const [remotePath, setRemotePath] = useState("/user");
  const [local, setLocal] = useState<FmEntry[]>([]);
  const [remote, setRemote] = useState<FmEntry[]>([]);
  const [selectedLocal, setSelectedLocal] = useState<string | null>(null);
  const [selectedRemote, setSelectedRemote] = useState<string | null>(null);

  const loadLocal = useCallback(
    async (path: string) => {
      if (!path) return;
      try {
        setLocal(await api.fmLocal(path));
      } catch (err) {
        setLocal([]);
        toast(`local: ${String(err)}`);
      }
    },
    [toast]
  );

  const loadRemote = useCallback(
    async (path: string) => {
      try {
        setRemote(await api.fmRemote(path));
      } catch (err) {
        setRemote([]);
        toast(`remote: ${String(err)}`);
      }
    },
    [toast]
  );

  useEffect(() => {
    if (!localPath) {
      void api.downloadsList().then((view) => {
        setLocalPath(view.dir);
        void loadLocal(view.dir);
      });
    } else {
      void loadLocal(localPath);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void loadRemote(remotePath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function parent(path: string): string {
    const clean = path.replace(/\/+$/, "");
    const idx = clean.lastIndexOf("/");
    if (idx <= 0) return "/";
    return clean.slice(0, idx);
  }

  return (
    <div className="page">
      <div className="panel">
        <div className="panel-head">
          <h2>File manager</h2>
          <div className="spacer" />
          <Btn
            small
            onClick={async () => {
              const picked = await pickers.pickDirectory("Choose a local folder");
              if (picked) {
                setLocalPath(picked);
                void loadLocal(picked);
              }
            }}
          >
            Pick local folder
          </Btn>
        </div>
        <div className="grid two-col">
          <div>
            <div className="field">
              <label>Local path</label>
              <input
                type="text"
                value={localPath}
                onChange={(e) => setLocalPath(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void loadLocal(localPath);
                }}
              />
            </div>
            <div className="quick">
              <Btn small onClick={() => void loadLocal(localPath)}>Refresh</Btn>
              <Btn small onClick={() => { const p = parent(localPath); setLocalPath(p); void loadLocal(p); }}>
                Up
              </Btn>
              <Btn
                small
                onClick={async () => {
                  const name = window.prompt("New folder name");
                  if (!name) return;
                  await api.fmMkdir(localPath, name);
                  void loadLocal(localPath);
                }}
              >
                New folder
              </Btn>
            </div>
            <div className="rows" style={{ marginTop: 10 }}>
              {local.length === 0 ? (
                <Empty title="Empty folder" />
              ) : (
                local.map((entry) => (
                  <div
                    className="row"
                    key={entry.path}
                    tabIndex={0}
                    aria-current={selectedLocal === entry.path}
                    onClick={() => setSelectedLocal(entry.path)}
                    onKeyDown={(e) => {
                      if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
                        e.preventDefault();
                        setSelectedLocal(entry.path);
                      }
                    }}
                    style={selectedLocal === entry.path ? { borderColor: "var(--line-laser)" } : undefined}
                  >
                    <div>
                      <div className="title">{entry.name}</div>
                      <div className="sub">
                        {entry.is_dir ? "folder" : fmtBytes(entry.size_bytes)}
                      </div>
                    </div>
                    <div className="grow" />
                    {entry.is_dir ? (
                      <Btn
                        small
                        onClick={() => {
                          setLocalPath(entry.path);
                          void loadLocal(entry.path);
                        }}
                      >
                        Open
                      </Btn>
                    ) : null}
                    <Btn
                      small
                      variant="primary"
                      onClick={async () => {
                        await api.fmUpload(entry.path, remotePath);
                        toast(`uploading ${entry.name}`);
                        reload();
                      }}
                    >
                      Upload
                    </Btn>
                  </div>
                ))
              )}
            </div>
          </div>

          <div>
            <div className="field">
              <label>Console path</label>
              <input
                type="text"
                value={remotePath}
                onChange={(e) => setRemotePath(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void loadRemote(remotePath);
                }}
              />
            </div>
            <div className="quick">
              <Btn small onClick={() => void loadRemote(remotePath)}>Refresh</Btn>
              <Btn small onClick={() => { const p = parent(remotePath); setRemotePath(p); void loadRemote(p); }}>
                Up
              </Btn>
              <Btn small onClick={() => { setRemotePath("/user"); void loadRemote("/user"); }}>
                /user
              </Btn>
              <Btn
                small
                onClick={async () => {
                  const name = window.prompt("New remote folder name");
                  if (!name) return;
                  await api.fmMkdirRemote(`${remotePath.replace(/\/$/, "")}/${name}`);
                  void loadRemote(remotePath);
                }}
              >
                New folder
              </Btn>
            </div>
            <div className="rows" style={{ marginTop: 10 }}>
              {remote.length === 0 ? (
                <Empty title="No entries" hint="Check the path and that FTP is running." />
              ) : (
                remote.map((entry) => (
                  <div
                    className="row"
                    key={entry.path}
                    tabIndex={0}
                    aria-current={selectedRemote === entry.path}
                    onClick={() => setSelectedRemote(entry.path)}
                    onKeyDown={(e) => {
                      if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
                        e.preventDefault();
                        setSelectedRemote(entry.path);
                      }
                    }}
                    style={selectedRemote === entry.path ? { borderColor: "var(--line-laser)" } : undefined}
                  >
                    <div>
                      <div className="title">{entry.name}</div>
                      <div className="sub">
                        {entry.is_dir ? "folder" : `${entry.kind} ${fmtBytes(entry.size_bytes)}`}
                      </div>
                    </div>
                    <div className="grow" />
                    {entry.is_dir ? (
                      <Btn
                        small
                        onClick={() => {
                          setRemotePath(entry.path);
                          void loadRemote(entry.path);
                        }}
                      >
                        Open
                      </Btn>
                    ) : null}
                    <Btn
                      small
                      variant="primary"
                      onClick={async () => {
                        await api.fmDownload(entry.path, localPath);
                        toast(`downloading ${entry.name}`);
                        reload();
                      }}
                    >
                      Download
                    </Btn>
                    <Btn
                      small
                      variant="danger"
                      onClick={async () => {
                        if (!confirmDialog(`Delete ${entry.name}?`, `${entry.path} is removed from the console.`)) return;
                        await api.fmDelete(entry.path, entry.is_dir, true);
                        void loadRemote(remotePath);
                      }}
                    >
                      Delete
                    </Btn>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
