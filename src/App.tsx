import { useEffect, useState } from "react";
import {
  core,
  devices,
  mirror,
  wireless,
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
 * Devices view. Discovery itself is native: the Rust core polls `adb` every
 * 2 s (watch_devices in commands.rs), keeps device state, and pushes a
 * `devices-changed` event whenever the picture changes. This component only
 * fetches the first snapshot, listens, and renders it in the reference
 * layout: Established Connections (cards), Active Discovery (watcher
 * status), Awaiting Authorization (unauthorized devices) and a service
 * status bar. Only fields the backend actually reports are shown — no IP,
 * battery, uptime, encoder, latency or FPS.
 */
function DevicesPanel() {
  const [snapshot, setSnapshot] = useState<DeviceSnapshot | null>(null);
  const [mirrorStatus, setMirrorStatus] = useState<MirrorStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | "usb" | "wifi">("all");
  const [pairOpen, setPairOpen] = useState(false);
  const [pairAddress, setPairAddress] = useState("");
  const [pairKey, setPairKey] = useState("");
  const [pairResult, setPairResult] = useState<
    { ok: boolean; text: string } | null
  >(null);
  const [pairBusy, setPairBusy] = useState(false);

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
    mirror
      .status()
      .then((next) => {
        if (alive) setMirrorStatus(next);
      })
      .catch(() => {
        // Mirror status is decorative here; the mirroring view reports it.
      });
    const unlistenDevices = devices.onChanged((next) => {
      if (alive) setSnapshot(next);
    });
    const unlistenMirror = mirror.onChanged((next) => {
      if (alive) setMirrorStatus(next);
    });
    return () => {
      alive = false;
      unlistenDevices.then((off) => off());
      unlistenMirror.then((off) => off());
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
  const visible = list.filter(
    (device) => filter === "all" || device.transport === filter,
  );
  const established = visible.filter((device) => device.state === "online");
  const awaiting = visible.filter((device) => device.state === "unauthorized");
  const notAttached = visible.length - established.length - awaiting.length;

  return (
    <>
      <header className="disc-header">
        <div className="disc-title-row">
          <h1 className="disc-title">Discovery</h1>
          <span className={adb.available && !adb.error ? "chip chip-active" : "chip"}>
            {adb.available && !adb.error ? "Active" : "Idle"}
          </span>
        </div>
        <div className="disc-controls">
          <div className="disc-filters" role="group" aria-label="Transport filter">
            {(
              [
                ["all", "All Devices"],
                ["usb", "USB Only"],
                ["wifi", "Wireless"],
              ] as const
            ).map(([value, label]) => (
              <button
                key={value}
                type="button"
                className={
                  filter === value ? "disc-filter is-active" : "disc-filter"
                }
                onClick={() => setFilter(value)}
              >
                {label}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="btn"
            onClick={() => {
              setPairOpen((open) => !open);
              setPairResult(null);
            }}
          >
            Manual Connect
          </button>
        </div>
      </header>

      {pairOpen && (
        <section className="panel">
          <div className="panel-head">
            <h2>Manual Connect</h2>
            <span className="chip">adb pair</span>
          </div>
          <form
            className="pair-form"
            onSubmit={(event) => {
              event.preventDefault();
              const address = pairAddress.trim();
              const key = pairKey.trim();
              if (!/^\d{1,3}(\.\d{1,3}){3}:\d+$/.test(address)) {
                setPairResult({
                  ok: false,
                  text: "Address must look like 192.168.1.42:37000.",
                });
                return;
              }
              if (!/^\d{4,8}$/.test(key)) {
                setPairResult({
                  ok: false,
                  text: "Pairing key is the 4–8 digit code shown on the device.",
                });
                return;
              }
              setPairBusy(true);
              setPairResult(null);
              wireless
                .pair(address, key)
                .then(() => {
                  setPairResult({
                    ok: true,
                    text: `Paired to ${address} — the device should appear above once authorized.`,
                  });
                })
                .catch((err) => {
                  setPairResult({ ok: false, text: String(err) });
                })
                .finally(() => setPairBusy(false));
            }}
          >
            <input
              type="text"
              placeholder="192.168.1.42:37000"
              value={pairAddress}
              onChange={(event) => setPairAddress(event.target.value)}
              aria-label="Device address"
            />
            <input
              type="text"
              placeholder="Pairing key"
              value={pairKey}
              onChange={(event) => setPairKey(event.target.value)}
              aria-label="Pairing key"
            />
            <button type="submit" className="btn btn-primary" disabled={pairBusy}>
              {pairBusy ? "Pairing…" : "Pair"}
            </button>
          </form>
          {pairResult && (
            <p
              className={
                pairResult.ok ? "pair-result is-ok" : "pair-result is-error"
              }
            >
              {pairResult.text}
            </p>
          )}
        </section>
      )}

      {adb.error && (
        <div className="banner">
          ADB problem — device detection is degraded.
          <div className="banner-detail">{adb.error}</div>
        </div>
      )}

      <section className="panel">
        <div className="panel-head">
          <h2>Established Connections</h2>
          <span className="chip">
            {established.length} Device{established.length === 1 ? "" : "s"}{" "}
            Available
          </span>
        </div>
        {established.length === 0 ? (
          <div className="disc-body">
            <p className="muted">
              No Android devices attached — connect one over USB and allow
              USB debugging.
            </p>
          </div>
        ) : (
          <div className="disc-grid">
            {established.map((device) => {
              const resolution =
                mirrorStatus?.running === true &&
                mirrorStatus.serial === device.serial &&
                mirrorStatus.width > 0 &&
                mirrorStatus.height > 0
                  ? `${mirrorStatus.width} × ${mirrorStatus.height} px`
                  : "not mirroring";
              return (
                <article className="device-card" key={device.serial}>
                  <div className="device-card-head">
                    <h3>{device.model ?? device.serial}</h3>
                    <span className="state">{device.state}</span>
                  </div>
                  <p className="device-id">{device.serial}</p>
                  <div className="stats">
                    <div className="stat">
                      <span className="stat-label">Manufacturer</span>
                      <span className="stat-value">
                        {device.manufacturer ?? "—"}
                      </span>
                    </div>
                    <div className="stat">
                      <span className="stat-label">Transport</span>
                      <span className="stat-value">{device.transport}</span>
                    </div>
                    <div className="stat">
                      <span className="stat-label">State</span>
                      <span className="stat-value">{device.state}</span>
                    </div>
                    <div className="stat">
                      <span className="stat-label">Mirror resolution</span>
                      <span className="stat-value">{resolution}</span>
                    </div>
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </section>

      <section className="panel">
        <div className="panel-head">
          <h2>Active Discovery</h2>
          <span className="chip">
            {adb.error
              ? "degraded"
              : adb.available
                ? `adb · ${adb.source ?? "ready"}`
                : "adb unavailable"}
          </span>
        </div>
        <div className="disc-body">
          <p>
            {adb.available && !adb.error
              ? "Scanning for device changes…"
              : "Discovery is not running."}
          </p>
          <p className="muted">
            Native watcher polls <code>adb devices</code> every 2 seconds and
            broadcasts a fresh snapshot whenever something changes.
          </p>
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <h2>Awaiting Authorization</h2>
          <span className="chip">{awaiting.length} pending</span>
        </div>
        {awaiting.length === 0 ? (
          <div className="disc-body">
            <p className="muted">
              No devices waiting — every attached device is authorized.
            </p>
          </div>
        ) : (
          awaiting.map((device) => (
            <div className="auth-item" key={device.serial}>
              <div>
                <h4>{device.model ?? device.serial}</h4>
                <p className="muted">
                  {device.serial} · confirm USB debugging on the device
                  screen.
                </p>
              </div>
              <span className="state">{device.state}</span>
            </div>
          ))
        )}
      </section>

      <footer className="statusbar">
        <span>
          Service: <strong>{adb.error ? "degraded" : "Ready"}</strong>
        </span>
        <span>
          ADB:{" "}
          <strong>
            {adb.available ? (adb.source ?? "ready") : "unavailable"}
          </strong>
        </span>
        <span>
          Watcher: <strong>2s poll</strong>
        </span>
        <span>
          Devices: <strong>{list.length} attached</strong>
          {notAttached > 0 && ` (${notAttached} offline/disconnected)`}
        </span>
        <span>
          Mirror:{" "}
          <strong>
            {mirrorStatus?.running === true ? mirrorStatus.phase : "idle"}
          </strong>
        </span>
      </footer>
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
