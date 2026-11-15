import { useCallback, useEffect, useState } from "react";
import type { PageProps } from "../App";
import { api } from "../api";
import type { FmEntry } from "../types";
import { Btn, Chip, Empty, confirmDialog, fmtBytes } from "../ui";

export default function Downloads({ toast, reload, refreshKey }: PageProps) {
  const [dir, setDir] = useState("");
  const [files, setFiles] = useState<FmEntry[]>([]);

  const load = useCallback(async () => {
    try {
      const view = await api.downloadsList();
      setDir(view.dir);
      setFiles(view.files);
    } catch (err) {
      toast(`could not list downloads: ${String(err)}`);
    }
  }, [toast]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  return (
    <div className="page">
      <div className="panel">
        <div className="panel-head">
          <h2>Downloads</h2>
          <div className="spacer" />
          <Btn small onClick={() => void load()}>
            Refresh
          </Btn>
        </div>
        <div className="mono" style={{ fontSize: 12, color: "var(--text-faint)", marginBottom: 10 }}>
          {dir}
        </div>

        {files.length === 0 ? (
          <Empty
            title="No downloads yet"
            hint="Games and homebrew download here first. Send a PKG, ISO or game folder to the console once it lands."
          />
        ) : (
          <div className="rows">
            {files.map((file) => (
              <div className="row" key={file.path}>
                <div>
                  <div className="title">{file.name}</div>
                  <div className="sub">
                    {file.kind} {file.is_dir ? "folder" : fmtBytes(file.size_bytes)}
                  </div>
                </div>
                <div className="grow" />
                <Chip tone={file.kind === "pkg" ? "laser" : file.kind === "iso" ? "good" : "neutral"}>
                  {file.kind}
                </Chip>
                <Btn small onClick={() => void api.downloadOpen(file.name).catch((e) => toast(String(e)))}>
                  Show
                </Btn>
                <Btn
                  small
                  variant="primary"
                  onClick={async () => {
                    try {
                      await api.downloadPush(file.name);
                      toast(`sending ${file.name} to the console`);
                      reload();
                    } catch (err) {
                      toast(`send failed: ${String(err)}`);
                    }
                  }}
                >
                  Send to PS5
                </Btn>
                <Btn
                  small
                  variant="danger"
                  onClick={async () => {
                    if (!confirmDialog(`Delete ${file.name}?`, "It is removed from the download folder.")) return;
                    await api.downloadDelete(file.name);
                    void load();
                  }}
                >
                  Delete
                </Btn>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
