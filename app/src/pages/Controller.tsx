import { useCallback, useEffect, useRef, useState } from "react";
import type { PageProps } from "../App";
import { api } from "../api";
import { Btn, Chip, Empty } from "../ui";

const KEY_MAP: Record<string, string> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "cross",
  Escape: "circle",
  Backspace: "square",
  KeyT: "triangle",
  KeyQ: "l1",
  KeyE: "r1",
  Digit1: "l2",
  Digit3: "r2",
  ShiftLeft: "l3",
  ShiftRight: "r3",
  Tab: "create",
  Space: "options",
  KeyF: "ps",
  KeyG: "touchpad",
};

const PRESETS: { label: string; buttons: string[] }[] = [
  { label: "L3 + R3", buttons: ["l3", "r3"] },
  { label: "CREATE + OPTIONS", buttons: ["create", "options"] },
  { label: "CREATE + CIRCLE", buttons: ["create", "circle"] },
  { label: "R2 + L3 + CIRCLE", buttons: ["r2", "l3", "circle"] },
];

export default function Controller({ snap, toast }: PageProps) {
  const active = snap.statuses.find((s) => s.host_id === snap.active) ?? null;
  const [held, setHeld] = useState<Set<string>>(new Set());
  const [combo, setCombo] = useState<string[]>([]);
  const [macro, setMacro] = useState("cross, down, down, cross");
  const [connected, setConnected] = useState(false);
  const [busy, setBusy] = useState(false);
  const heldRef = useRef<Set<string>>(new Set());

  const ready = connected;

  useEffect(() => {
    void api
      .padStatus()
      .then((status) => setConnected(status.connected))
      .catch(() => setConnected(false));
  }, []);

  // Release one button, or every button when no name is given.
  const release = useCallback(async (button?: string) => {
    if (button) {
      if (!heldRef.current.has(button)) return;
      heldRef.current.delete(button);
      setHeld(new Set(heldRef.current));
      try {
        await api.padRelease(button);
      } catch {
        /* the payload may have gone away */
      }
      return;
    }
    if (heldRef.current.size > 0) {
      heldRef.current = new Set();
      setHeld(new Set());
      try {
        await api.padRelease();
      } catch {
        /* the payload may have gone away */
      }
    }
  }, []);

  const hold = useCallback(
    async (button: string) => {
      if (!ready) {
        toast("connect the pad first; Ghostpad must run on the console");
        return;
      }
      heldRef.current.add(button);
      setHeld(new Set(heldRef.current));
      try {
        await api.padHold(button);
      } catch (err) {
        toast(`pad failed: ${String(err)}`);
        heldRef.current.delete(button);
        setHeld(new Set(heldRef.current));
        setConnected(false);
      }
    },
    [ready, toast]
  );

  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      const button = KEY_MAP[e.code];
      if (!button || e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
      e.preventDefault();
      void hold(button);
    };
    const up = (e: KeyboardEvent) => {
      const button = KEY_MAP[e.code];
      if (button) void release(button);
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      void release();
    };
  }, [hold, release]);

  async function connect() {
    setBusy(true);
    try {
      await api.padConnect();
      setConnected(true);
      toast("Ghostpad connected");
    } catch (err) {
      setConnected(false);
      toast(`connect failed: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    await release();
    await api.padDisconnect();
    setConnected(false);
    toast("pad disconnected");
  }

  async function fireCombo(buttons: string[]) {
    if (!ready) {
      toast("connect the pad first");
      return;
    }
    try {
      await api.padCombo(buttons, 140);
      toast(`combo ${buttons.join("+")}`);
    } catch (err) {
      toast(`combo failed: ${String(err)}`);
    }
  }

  return (
    <div className="page">
      <div className="panel">
        <div className="panel-head">
          <h2>Virtual controller</h2>
          <div className="spacer" />
          {connected ? (
            <Chip tone="good" live>
              connected to {active?.name ?? "console"}
            </Chip>
          ) : (
            <Chip tone="warn">not connected</Chip>
          )}
          <Btn small variant={connected ? "ghost" : "primary"} disabled={busy} onClick={() => void connect()}>
            {busy ? "Connecting..." : connected ? "Reconnect" : "Connect pad"}
          </Btn>
          {connected ? (
            <Btn small onClick={() => void disconnect()}>
              Disconnect
            </Btn>
          ) : null}
        </div>

        {!connected ? (
          <Empty
            title="Ghostpad is not connected"
            hint="Load the Ghostpad payload on the console from the Payloads page, then press Connect. The payload listens on TCP 6967."
            action={
              <Btn variant="primary" disabled={busy} onClick={() => void connect()}>
                Connect pad
              </Btn>
            }
          />
        ) : null}

        <div className="pad-layout">
          <div className={`pad ${connected ? "" : "off"}`} aria-disabled={!connected}>
            <div className="pad-shoulders">
              <PadButton id="l1" label="L1" held={held} hold={hold} release={release} />
              <PadButton id="l2" label="L2" held={held} hold={hold} release={release} />
              <PadButton id="l3" label="L3" held={held} hold={hold} release={release} />
              <PadButton id="r1" label="R1" held={held} hold={hold} release={release} />
              <PadButton id="r2" label="R2" held={held} hold={hold} release={release} />
              <PadButton id="r3" label="R3" held={held} hold={hold} release={release} />
            </div>

            <div className="pad-main">
              <div className="dpad">
                <PadButton id="up" label="Up" held={held} hold={hold} release={release} className="du" />
                <PadButton id="left" label="Left" held={held} hold={hold} release={release} className="dl" />
                <PadButton id="right" label="Right" held={held} hold={hold} release={release} className="dr" />
                <PadButton id="down" label="Down" held={held} hold={hold} release={release} className="dd" />
              </div>
              <div className="face">
                <PadButton id="triangle" label="Triangle" held={held} hold={hold} release={release} className="ft" />
                <PadButton id="square" label="Square" held={held} hold={hold} release={release} className="fs" />
                <PadButton id="circle" label="Circle" held={held} hold={hold} release={release} className="fc" />
                <PadButton id="cross" label="Cross" held={held} hold={hold} release={release} className="fx" />
              </div>
            </div>

            <div className="pad-center">
              <PadButton id="create" label="Create" held={held} hold={hold} release={release} />
              <PadButton id="ps" label="PS" held={held} hold={hold} release={release} />
              <PadButton id="options" label="Options" held={held} hold={hold} release={release} />
            </div>

            <div className="pad-sticks">
              <div>
                <div className="stick-label">Left stick</div>
                <Analog side="L" ready={ready} />
              </div>
              <div>
                <div className="stick-label">Right stick</div>
                <Analog side="R" ready={ready} />
              </div>
            </div>
          </div>

          <div className="pad-side">
            <div className="panel" style={{ margin: 0 }}>
              <div className="panel-head">
                <h3>Combos</h3>
              </div>
              <div className="quick">
                {PRESETS.map((preset) => (
                  <Btn key={preset.label} small onClick={() => void fireCombo(preset.buttons)}>
                    {preset.label}
                  </Btn>
                ))}
              </div>
              <div className="panel-head" style={{ marginTop: 16 }}>
                <h3>Build a combo</h3>
              </div>
              <div className="quick">
                {["cross", "circle", "square", "triangle", "l1", "r1", "l2", "r2", "l3", "r3", "create", "options"].map(
                  (button) => (
                    <button
                      key={button}
                      className={`chip ${combo.includes(button) ? "laser" : ""}`}
                      onClick={() =>
                        setCombo((cur) =>
                          cur.includes(button) ? cur.filter((b) => b !== button) : [...cur, button]
                        )
                      }
                    >
                      {button}
                    </button>
                  )
                )}
              </div>
              <div className="quick" style={{ marginTop: 10 }}>
                <Btn
                  variant="primary"
                  disabled={combo.length === 0}
                  onClick={() => void fireCombo(combo)}
                >
                  Press {combo.join("+") || "nothing"}
                </Btn>
                <Btn small onClick={() => setCombo([])}>
                  Clear
                </Btn>
              </div>

              <div className="panel-head" style={{ marginTop: 16 }}>
                <h3>Macro</h3>
              </div>
              <p style={{ marginTop: 0, fontSize: 12.5, color: "var(--text-faint)" }}>
                Comma-separated presses. Use <span className="mono">a+b</span> for a chord.
              </p>
              <input
                type="text"
                className="search"
                style={{ width: "100%" }}
                value={macro}
                onChange={(e) => setMacro(e.target.value)}
              />
              <div className="quick" style={{ marginTop: 10 }}>
                <Btn
                  onClick={async () => {
                    try {
                      await api.padMacro(macro);
                      toast("macro sent");
                    } catch (err) {
                      toast(`macro failed: ${String(err)}`);
                    }
                  }}
                >
                  Run macro
                </Btn>
                <Btn onClick={() => setMacro("up, up, cross, down, cross")}>Sample</Btn>
              </div>

              <div className="panel-head" style={{ marginTop: 16 }}>
                <h3>Payload</h3>
              </div>
              <p style={{ marginTop: 0, fontSize: 12.5, color: "var(--text-faint)" }}>
                The pad is a virtual DualSense registered by Ghostpad. Touchpad
                and rumble are not sent by the payload.
              </p>
              <p style={{ fontSize: 12, color: "var(--text-faint)" }}>
                Keys: arrows d-pad, Enter cross, Esc circle, Backspace square, T
                triangle, Q/E bumpers, 1/3 triggers, Shift sticks, Tab Create,
                Space Options, F PS, G touchpad.
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function PadButton({
  id,
  label,
  held,
  hold,
  release,
  className = "",
}: {
  id: string;
  label: string;
  held: Set<string>;
  hold: (button: string) => Promise<void>;
  release: (button?: string) => Promise<void>;
  className?: string;
}) {
  return (
    <button
      className={`pad-btn ${className} ${held.has(id) ? "on" : ""}`}
      onPointerDown={(e) => {
        e.preventDefault();
        void hold(id);
      }}
      onPointerUp={() => void release(id)}
      onPointerLeave={() => void release(id)}
      onPointerCancel={() => void release(id)}
      aria-label={label}
      title={label}
    >
      {label}
    </button>
  );
}

function Analog({ side, ready }: { side: string; ready: boolean }) {
  const dirs = ["up_left", "up", "up_right", "left", "right", "down_left", "down", "down_right"];
  return (
    <div className="analog">
      {dirs.map((direction) => (
        <button
          key={direction}
          className="pad-btn small"
          disabled={!ready}
          onClick={async () => {
            try {
              await api.padAnalog(side, direction);
            } catch {
              /* handled by the page-level toast for presses */
            }
          }}
        >
          {direction.replace("_", " ")}
        </button>
      ))}
    </div>
  );
}
