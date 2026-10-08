import { useEffect, useState } from "react";
import {
  core,
  devices,
  mirror,
  type AppInfo,
  type ModuleInfo,
  type DeviceSnapshot,
  type MirrorStatus,
} from "./lib/tauri";
import "./App.css";

type View = "dashboard" | "devices" | "mirroring" | "settings";

const NAV: { id: View; label: string; ready: boolean }[] = [
  { id: "dashboard", label: "Dashboard", ready: true },
  { id: "devices", label: "Devices", ready: true },
  { id: "mirroring", label: "Mirroring", ready: true },
  { id: "settings", label: "Settings", ready: false },
];

type Load =
  | { status: "loading" }
  | { status: "ready"; app: AppInfo; modules: ModuleInfo[] }
  | { status: "error"; message: string };

function App() {
  const [load, setLoad] = useState<Load>({ status: "loading" });
  const [view, setView] = useState<View>("dashboard");

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [app, modules] = await Promise.all([
          core.appInfo(),
          core.coreStatus(),
        ]);
        if (alive) setLoad({ status: "ready", app, modules });
      } catch (err) {
        if (alive) setLoad({ status: "error", message: String(err) });
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">H</span>
          <span className="brand-text">
            <strong>HermesGate</strong>
            <small>by Lex</small>
          </span>
        </div>

        <nav className="nav" aria-label="Primary">
          {NAV.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`nav-item${view === item.id ? " is-active" : ""}`}
              disabled={!item.ready}
              onClick={() => setView(item.id)}
            >
              <span>{item.label}</span>
              {!item.ready && <span className="nav-tag">Soon</span>}
            </button>
          ))}
        </nav>

        <div className="sidebar-foot">
          <span>HermesGate — by Lex</span>
          {load.status === "ready" && <span>v{load.app.version}</span>}
        </div>
      </aside>

      <main className="main">
        <header className="topbar">
          <h1>{NAV.find((item) => item.id === view)?.label ?? "Dashboard"}</h1>
          {load.status === "ready" && (
            <span className="chip">
              {load.app.platform} / {load.app.arch}
            </span>
          )}
        </header>

        <div className="content">
          {view === "dashboard" && load.status === "loading" && (
            <p className="muted">Contacting the native core…</p>
          )}

          {view === "dashboard" && load.status === "error" && (
            <div className="banner">
              Native bridge unavailable — this UI must run inside the
              HermesGate shell (<code>npm run tauri dev</code>).
              <div className="banner-detail">{load.message}</div>
            </div>
          )}

          {view === "dashboard" && load.status === "ready" && (
            <>
              <section className="cards">
                <article className="card">
                  <h2>Application</h2>
                  <p className="card-value">{load.app.name}</p>
                  <p className="muted">version {load.app.version}</p>
                </article>
                <article className="card">
                  <h2>Runtime</h2>
                  <p className="card-value">
                    {load.app.platform} / {load.app.arch}
                  </p>
                  <p className="muted">Rust native core</p>
                </article>
                <article className="card">
                  <h2>Native core</h2>
                  <p className="card-value">{load.modules.length} modules</p>
                  <p className="muted">boundaries registered</p>
                </article>
              </section>

              <section className="panel">
                <div className="panel-head">
                  <h2>Core modules</h2>
                  <span className="muted">
                    Boundaries are in place; implementations follow the roadmap.
                  </span>
                </div>
                <table className="table">
                  <thead>
                    <tr>
                      <th>Module</th>
                      <th>Responsibility</th>
                      <th className="col-state">State</th>
                    </tr>
                  </thead>
                  <tbody>
                    {load.modules.map((m) => (
                      <tr key={m.id}>
                        <td>{m.name}</td>
                        <td className="muted">{m.responsibility}</td>
                        <td className="col-state">
                          <span className="state">{m.state}</span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </section>
            </>
          )}

          {view === "devices" && <DevicesPanel />}
          {view === "mirroring" && <MirroringPanel />}
        </div>
      </main>
    </div>
  );
}

/**
 * Devices view. Discovery itself is native: the Rust core polls `adb`,
 * keeps device state, and pushes a `devices-changed` event whenever the
 * picture changes. This component only fetches the first snapshot, listens,
 * and renders it.
 */
function DevicesPanel() {
  const [snapshot, setSnapshot] = useState<DeviceSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    devices
      .list()
      .then((next) => {
        if (alive) setSnapshot(next);
      })
      .catch((err) => {
        if (alive) setError(String(err));
      });
    const unlisten = devices.onChanged((next) => {
      if (alive) setSnapshot(next);
    });
    return () => {
      alive = false;
      unlisten.then((off) => off());
    };
  }, []);

  if (error) {
    return (
      <div className="banner">
        Device service unavailable — the native core did not answer.
        <div className="banner-detail">{error}</div>
      </div>
    );
  }

  if (!snapshot) {
    return <p className="muted">Reading devices…</p>;
  }

  const { adb, devices: list } = snapshot;
  return (
    <>
      {adb.error && (
        <div className="banner">
          ADB problem — device detection is degraded.
          <div className="banner-detail">{adb.error}</div>
        </div>
      )}

      <section className="panel">
        <div className="panel-head">
          <h2>Android devices</h2>
          <span className="chip">
            {adb.error
              ? "adb error"
              : adb.available
                ? `adb · ${adb.source ?? "ready"}`
                : "adb unavailable"}{" "}
            · {list.length} attached
          </span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>Device</th>
              <th>Model</th>
              <th>Android</th>
              <th>Transport</th>
              <th className="col-state">State</th>
            </tr>
          </thead>
          <tbody>
            {list.length === 0 ? (
              <tr>
                <td className="muted" colSpan={5}>
                  {adb.error
                    ? "No devices: the ADB call failed (see the banner above)."
                    : "No Android devices attached — connect one over USB and allow USB debugging."}
                </td>
              </tr>
            ) : (
              list.map((device) => (
                <tr key={device.serial}>
                  <td>
                    {device.serial}
                    {device.manufacturer && <p className="muted">{device.manufacturer}</p>}
                  </td>
                  <td className="muted">{device.model ?? "—"}</td>
                  <td className="muted">{device.androidVersion ?? "—"}</td>
                  <td className="muted">{device.transport}</td>
                  <td className="col-state">
                    <span className="state">{device.state}</span>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </section>
    </>
  );
}

/**
 * Mirroring view. The engine itself is native: `start`/`stop` drive one
 * session for a selected device and the core pushes `mirror-changed` on
 * every transition. This component only lists usable devices, forwards the
 * commands, and renders the live status.
 */
function MirroringPanel() {
  const [snapshot, setSnapshot] = useState<DeviceSnapshot | null>(null);
  const [status, setStatus] = useState<MirrorStatus | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [mirrorError, setMirrorError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    devices
      .list()
      .then((next) => {
        if (alive) setSnapshot(next);
      })
      .catch((err) => {
        if (alive) setDeviceError(String(err));
      });
    mirror
      .status()
      .then((next) => {
        if (alive) setStatus(next);
      })
      .catch((err) => {
        if (alive) setMirrorError(String(err));
      });
    const unlistenDevices = devices.onChanged((next) => {
      if (alive) setSnapshot(next);
    });
    const unlistenMirror = mirror.onChanged((next) => {
      if (alive) setStatus(next);
    });
    return () => {
      alive = false;
      unlistenDevices.then((off) => off());
      unlistenMirror.then((off) => off());
    };
  }, []);

  const list = snapshot?.devices ?? [];
  // Only a device in state "online" (adb reported `device`) can be mirrored;
  // deriving the target here drops the selection the moment it stops being one.
  const target =
    list.find((device) => device.serial === selected && device.state === "online") ?? null;
  const running = status?.running === true;
  const busy = running || status?.phase === "starting";

  const start = async () => {
    if (!target) return;
    setMirrorError(null);
    try {
      await mirror.start(target.serial);
    } catch (err) {
      setMirrorError(String(err));
    }
  };

  const stop = async () => {
    setMirrorError(null);
    try {
      await mirror.stop();
    } catch (err) {
      setMirrorError(String(err));
    }
  };

  return (
    <>
      {deviceError && (
        <div className="banner">
          Device service unavailable — the native core did not answer.
          <div className="banner-detail">{deviceError}</div>
        </div>
      )}

      <section className="panel">
        <div className="panel-head">
          <h2>Select a device</h2>
          <span className="chip">
            {snapshot ? `${list.length} attached` : "reading…"}
          </span>
        </div>
        <table className="table">
          <thead>
            <tr>
              <th>Device</th>
              <th>Model</th>
              <th>Transport</th>
              <th className="col-state">State</th>
            </tr>
          </thead>
          <tbody>
            {!snapshot ? (
              <tr>
                <td className="muted" colSpan={4}>
                  Reading devices…
                </td>
              </tr>
            ) : list.length === 0 ? (
              <tr>
                <td className="muted" colSpan={4}>
                  No Android devices attached — connect one over USB and allow USB debugging.
                </td>
              </tr>
            ) : (
              list.map((device) => {
                const usable = device.state === "online";
                const isTarget = target?.serial === device.serial;
                return (
                  <tr
                    key={device.serial}
                    className={`device-row${isTarget ? " is-selected" : ""}${
                      usable ? "" : " is-disabled"
                    }`}
                    aria-selected={isTarget}
                    onClick={usable ? () => setSelected(device.serial) : undefined}
                  >
                    <td>
                      {device.serial}
                      {device.manufacturer && <p className="muted">{device.manufacturer}</p>}
                    </td>
                    <td className="muted">{device.model ?? "—"}</td>
                    <td className="muted">{device.transport}</td>
                    <td className="col-state">
                      <span className="state">{device.state}</span>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
        {snapshot && list.some((device) => device.state !== "online") && (
          <p className="muted mirror-hint">
            Only devices in the <code>online</code> state (USB debugging allowed) can be mirrored.
          </p>
        )}
      </section>

      {mirrorError && (
        <div className="banner">
          Mirroring command failed.
          <div className="banner-detail">{mirrorError}</div>
        </div>
      )}

      <section className="panel">
        <div className="panel-head">
          <h2>Mirror session</h2>
          <span className="chip">{status ? status.phase : "unknown"}</span>
        </div>
        <div className="mirror-body">
          <div className="mirror-status">
            {!status ? (
              <span className="muted">Reading mirror status…</span>
            ) : (
              <>
                <span className="state">{status.phase}</span>
                {status.reason && <span className="muted">{status.reason}</span>}
                {status.width > 0 && status.height > 0 && (
                  <span className="muted">
                    {status.width} × {status.height} px
                  </span>
                )}
                {status.serial && <span className="muted">{status.serial}</span>}
              </>
            )}
          </div>
          <div className="mirror-actions">
            <button
              type="button"
              className="btn btn-primary"
              disabled={!target || busy}
              onClick={start}
            >
              Start Mirror
            </button>
            <button type="button" className="btn" disabled={!running} onClick={stop}>
              Stop Mirror
            </button>
          </div>
        </div>
      </section>
    </>
  );
}

export default App;
