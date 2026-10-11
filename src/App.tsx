/**
 * HermesGate shell.
 *
 * One workflow, one page: the Devices page is where a device is picked, where
 * the mirroring mode is chosen, and where the session starts and stops. There
 * is no second control surface for the same session — Start/Stop is one
 * contextual button per device, and it drives the real engine.
 *
 * Everything native lives in Rust: device discovery (2 s `adb` poll), the
 * mirror engine and the audio engine, each pushed to this UI as an event. The
 * webview renders what the core reports and invents no field of its own.
 */
import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import {
  core,
  devices,
  mirror,
  wireless,
  type AppInfo,
  type DeviceSnapshot,
  type MirrorMode,
  type MirrorStatus,
  type ModuleInfo,
  type Transport,
} from "./lib/tauri";
import "./App.css";

type View = "dashboard" | "devices" | "settings";

/** Icon rail (reference sidebar is a 64 px strip of icons): the name lives in
 * the tooltip and the accessible label. */
const NAV: { id: View; label: string; ready: boolean; icon: ReactNode }[] = [
  { id: "dashboard", label: "Dashboard", ready: true, icon: <IconGauge /> },
  { id: "devices", label: "Devices", ready: true, icon: <IconPhone /> },
  { id: "settings", label: "Settings", ready: false, icon: <IconSliders /> },
];

/**
 * Mirroring modes, in the order the Device page lists them. The knobs behind
 * each one live in `core::mirroring::MirrorQuality::preset`; the hint states
 * exactly what the engine is asked for, so the picker never promises more than
 * it sets.
 */
const MODES: { id: MirrorMode; label: string; hint: string }[] = [
  {
    id: "sharp",
    label: "Sharp",
    hint: "Native resolution · 8 Mbit/s · uncapped fps (engine default)",
  },
  { id: "balanced", label: "Balanced", hint: "1280 px longest edge · 6 Mbit/s · 60 fps" },
  { id: "smooth", label: "Smooth", hint: "1024 px longest edge · 4 Mbit/s · 60 fps" },
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
        const [app, modules] = await Promise.all([core.appInfo(), core.coreStatus()]);
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
      <aside className="rail">
        <div className="rail-brand" title="HermesGate">
          H
        </div>
        <nav className="rail-nav" aria-label="Primary">
          {NAV.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`rail-item${view === item.id ? " is-active" : ""}`}
              disabled={!item.ready}
              aria-label={item.label}
              aria-current={view === item.id ? "page" : undefined}
              title={item.ready ? item.label : `${item.label} — coming soon`}
              onClick={() => setView(item.id)}
            >
              {item.icon}
            </button>
          ))}
        </nav>
        <div className="rail-foot" title={load.status === "ready" ? load.app.name : "HermesGate"}>
          {load.status === "ready" ? `v${load.app.version}` : "—"}
        </div>
      </aside>

      <main className="main">
        <header className="topbar">
          {/* Window title, not a page heading: the page heading lives on the
              page itself (reference keeps the two apart). */}
          <h1>{load.status === "ready" ? load.app.name : "HermesGate"}</h1>
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
              Native bridge unavailable — this UI must run inside the HermesGate
              shell (<code>npm run tauri dev</code>).
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

          {view === "devices" && <DevicePage />}
        </div>

        <footer className="statusbar">
          <span>
            App:{" "}
            <strong>
              HermesGate {load.status === "ready" ? `v${load.app.version}` : "—"}
            </strong>
          </span>
          <span>
            Platform:{" "}
            <strong>
              {load.status === "ready" ? `${load.app.platform} / ${load.app.arch}` : "—"}
            </strong>
          </span>
          <span>
            Core:{" "}
            <strong>{load.status === "ready" ? `${load.modules.length} modules` : "—"}</strong>
          </span>
          <span>
            View: <strong>{view}</strong>
          </span>
        </footer>
      </main>
    </div>
  );
}

/** One label/value cell of a telemetry grid. */
function Stat({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="stat">
      <span className="stat-label">{label}</span>
      <span className="stat-value">{value}</span>
    </div>
  );
}

/** `usb` → `USB`, `wifi` → `Wi-Fi`. */
function transportName(transport: Transport): string {
  return transport === "usb" ? "USB" : "Wi-Fi";
}

/**
 * Devices page — the whole mirroring workflow.
 *
 * Native facts only: the device snapshot (`devices-changed`, 2 s poll), the
 * mirror session (`mirror-changed`, 1 s poll) and the commands that drive it.
 * A device appears once: the Rust core groups an `adb` serial, its mDNS alias
 * and its `ip:port` into one entry (`core::device`), and this page renders one
 * card per entry with one contextual Start/Stop button.
 */
function DevicePage() {
  const [snapshot, setSnapshot] = useState<DeviceSnapshot | null>(null);
  const [status, setStatus] = useState<MirrorStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [commandError, setCommandError] = useState<string | null>(null);
  const [mode, setMode] = useState<MirrorMode>("sharp");
  // Transport the user picked per card, keyed by the card's stable address.
  // Absent while there is no choice to make: a single-transport device is used
  // on that transport without asking.
  const [chosen, setChosen] = useState<Record<string, Transport>>({});
  const [pending, setPending] = useState(false);
  const [filter, setFilter] = useState<"all" | "usb" | "wifi">("all");
  const [pairOpen, setPairOpen] = useState(false);
  const [pairAddress, setPairAddress] = useState("");
  const [pairKey, setPairKey] = useState("");
  const [pairResult, setPairResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [pairBusy, setPairBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    devices
      .list()
      .then((next) => {
        if (alive) setSnapshot(next);
      })
      .catch((err) => {
        if (alive) setLoadError(String(err));
      });
    mirror
      .status()
      .then((next) => {
        if (alive) setStatus(next);
      })
      .catch(() => {
        // The command errors are reported on the action that raised them.
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

  const start = async (serial: string) => {
    setCommandError(null);
    setPending(true);
    try {
      await mirror.start(serial, mode);
    } catch (err) {
      setCommandError(String(err));
    } finally {
      setPending(false);
    }
  };

  const stop = async () => {
    setCommandError(null);
    setPending(true);
    try {
      await mirror.stop();
    } catch (err) {
      setCommandError(String(err));
    } finally {
      setPending(false);
    }
  };

  const sendKey = async (key: "back" | "home" | "recents") => {
    setCommandError(null);
    try {
      await mirror.key(key);
    } catch (err) {
      setCommandError(String(err));
    }
  };

  if (loadError) {
    return (
      <div className="banner">
        Device service unavailable — the native core did not answer.
        <div className="banner-detail">{loadError}</div>
      </div>
    );
  }

  if (!snapshot) {
    return <p className="muted">Reading devices…</p>;
  }

  const { adb, devices: list } = snapshot;
  const sessionActive = status?.running === true || status?.phase === "starting";
  const sessionSerial = sessionActive ? (status?.serial ?? "") : "";
  const visible = list.filter(
    (device) =>
      filter === "all" ||
      device.endpoints.some((endpoint) => endpoint.transport === filter),
  );
  const established = visible.filter((device) => device.state === "online");
  const awaiting = visible.filter((device) => device.state === "unauthorized");
  const offline = visible.length - established.length - awaiting.length;
  const modeHint = MODES.find((entry) => entry.id === mode)?.hint ?? "";

  return (
    <>
      <header className="page-head">
        <div className="page-title-row">
          <h1 className="page-title">Devices</h1>
          <span className={adb.available && !adb.error ? "chip chip-active" : "chip"}>
            {adb.available && !adb.error ? "Discovery active" : "Discovery idle"}
          </span>
        </div>
        <div className="page-controls">
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
                className={filter === value ? "disc-filter is-active" : "disc-filter"}
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {label}
              </button>
            ))}
          </div>
          <button
            type="button"
            className={pairOpen ? "btn btn-quiet is-on" : "btn"}
            aria-expanded={pairOpen}
            onClick={() => {
              setPairOpen((open) => !open);
              setPairResult(null);
            }}
          >
            Manual Connect
          </button>
        </div>
      </header>

      {adb.error && (
        <div className="banner">
          ADB problem — device detection is degraded.
          <div className="banner-detail">{adb.error}</div>
        </div>
      )}

      {commandError && (
        <div className="banner">
          Command failed — the session was not changed.
          <div className="banner-detail">{commandError}</div>
        </div>
      )}

      {pairOpen && (
        <section className="section">
          <div className="section-label">
            <h2>Manual Connect</h2>
            <span className="section-note">adb pair · wireless debugging</span>
          </div>
          <div className="panel">
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
                      text: `Paired to ${address} — the device appears above once authorized.`,
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
              <p className={pairResult.ok ? "pair-result is-ok" : "pair-result is-error"}>
                {pairResult.text}
              </p>
            )}
          </div>
        </section>
      )}

      <section className="section">
        <div className="section-label">
          <h2>Device</h2>
          <span className="section-note">
            {established.length === 0
              ? "none attached"
              : `${established.length} attached${offline > 0 ? ` · ${offline} offline` : ""}`}
          </span>
        </div>

        {established.length === 0 ? (
          <div className="panel">
            <div className="disc-body">
              <p>No Android device is attached.</p>
              <p className="muted">
                Plug the phone in over USB and allow USB debugging, or use{" "}
                <strong>Manual Connect</strong> above to pair it over Wi-Fi.
              </p>
            </div>
          </div>
        ) : (
          established.map((device) => {
            // The transport in use: the user's pick while it is still
            // available, otherwise the first one the device answers on (a
            // device that lost USB falls back to Wi-Fi without a stale pick).
            const endpoint =
              device.endpoints.find((entry) => entry.transport === chosen[device.serial]) ??
              device.endpoints[0];
            // The session reports the serial it was started with, so the card
            // is the one that owns the session when any of its endpoints
            // matches — never by model name.
            const onThisDevice =
              sessionSerial !== "" &&
              device.endpoints.some((entry) => entry.serial === sessionSerial);
            const starting = sessionActive && status?.phase === "starting" && onThisDevice;
            const running = status?.running === true && onThisDevice;
            const otherBusy = sessionActive && !onThisDevice;
            const failedHere = status?.phase === "failed" && onThisDevice;
            const canToggle =
              !pending &&
              !starting &&
              !otherBusy &&
              endpoint !== undefined &&
              (running || (device.state === "online" && adb.available));
            const hint = starting
              ? "Opening the session…"
              : otherBusy
                ? `Another device (${sessionSerial}) is mirroring — stop it first.`
                : running
                  ? "Session live — the mode applies to the next start."
                  : failedHere && status?.reason
                    ? status.reason
                    : !adb.available
                      ? "ADB runtime unavailable — mirroring is disabled."
                      : modeHint;

            return (
              <article className="device-hero" key={device.serial}>
                <div className="device-hero-top">
                  <div className="device-ident">
                    <div className="device-glyph">
                      <IconPhone size={30} />
                      <span className={`glyph-dot is-${device.state}`} />
                    </div>
                    <div className="device-names">
                      <div className="device-name-row">
                        <h3>{device.model ?? device.serial}</h3>
                        <span className="state">{device.state}</span>
                      </div>
                      <div className="device-meta">
                        <span>{device.serial}</span>
                        {device.manufacturer && <span>{device.manufacturer}</span>}
                        {device.androidVersion && <span>Android {device.androidVersion}</span>}
                        <span>
                          {device.endpoints.length === 0
                            ? "no transport"
                            : device.endpoints
                                .map((entry) => transportName(entry.transport))
                                .join(" + ")}
                        </span>
                      </div>
                    </div>
                  </div>

                  <div className="device-action">
                    {device.endpoints.length === 0 ? (
                      <p className="device-endpoint is-none">
                        No transport available — nothing to mirror over yet.
                      </p>
                    ) : (
                      <>
                        <div className="mode-picker">
                          <span className="mode-label">Connection</span>
                          <div
                            className="disc-filters"
                            role="group"
                            aria-label={`Connection transport for ${device.model ?? device.serial}`}
                          >
                            {device.endpoints.map((entry) => (
                              <button
                                key={entry.transport}
                                type="button"
                                className={
                                  entry.transport === endpoint?.transport
                                    ? "disc-filter is-active"
                                    : "disc-filter"
                                }
                                // A running session keeps the transport it was
                                // started with: switching is a change of the
                                // next start, never of the live connection.
                                disabled={sessionActive}
                                aria-pressed={entry.transport === endpoint?.transport}
                                title={`${transportName(entry.transport)} · ${entry.serial}`}
                                onClick={() =>
                                  setChosen((previous) => ({
                                    ...previous,
                                    [device.serial]: entry.transport,
                                  }))
                                }
                              >
                                {transportName(entry.transport)}
                              </button>
                            ))}
                          </div>
                        </div>
                        <p className="device-endpoint">
                          {transportName(endpoint!.transport)} via {endpoint!.serial}
                        </p>
                      </>
                    )}
                    <div className="mode-picker">
                      <span className="mode-label">Mode</span>
                      <div className="disc-filters" role="group" aria-label="Mirroring mode">
                        {MODES.map((entry) => (
                          <button
                            key={entry.id}
                            type="button"
                            className={mode === entry.id ? "disc-filter is-active" : "disc-filter"}
                            disabled={sessionActive}
                            aria-pressed={mode === entry.id}
                            title={entry.hint}
                            onClick={() => setMode(entry.id)}
                          >
                            {entry.label}
                          </button>
                        ))}
                      </div>
                    </div>
                    <button
                      type="button"
                      className={`btn btn-lg ${running ? "btn-stop" : "btn-primary"}`}
                      disabled={!canToggle}
                      onClick={() => (running ? stop() : start(endpoint!.serial))}
                    >
                      {starting ? "Starting…" : running ? "Stop Mirroring" : "Start Mirroring"}
                    </button>
                    <p className="device-action-hint">{hint}</p>
                  </div>
                </div>

                <div className="device-stats">
                  <Stat
                    label="Mirror"
                    value={
                      running
                        ? "running"
                        : starting
                          ? "starting"
                          : failedHere
                            ? "failed"
                            : otherBusy
                              ? "other device"
                              : "idle"
                    }
                  />
                  <Stat
                    label="Resolution"
                    value={
                      running && (status?.width ?? 0) > 0
                        ? `${status?.width} × ${status?.height} px`
                        : "—"
                    }
                  />
                  <Stat
                    label="Decode FPS"
                    value={running && status?.stats ? status.stats.fps.toFixed(1) : "—"}
                  />
                  <Stat
                    label="Received"
                    value={
                      running && status?.stats
                        ? `${((status.stats.bytesPerS * 8) / 1e6).toFixed(1)} Mbit/s`
                        : "—"
                    }
                  />
                  <Stat
                    label="Audio"
                    value={
                      running && status?.audio
                        ? status.audio.error
                          ? "unavailable"
                          : `${(status.audio.sampleRate / 1000).toFixed(0)} kHz · ${Math.round(status.audio.kbps)} kbit/s`
                        : "—"
                    }
                  />
                </div>
              </article>
            );
          })
        )}
      </section>

      <section className="section">
        <div className="section-label">
          <h2>Mirror Session</h2>
          {/* One phase readout for this panel, not two: the badge that used to
              repeat it inside the row is gone, and a failure is emphasised
              here instead of being lost among the counters. */}
          <span className={status?.phase === "failed" ? "section-note is-error" : "section-note"}>
            {status ? status.phase : "unknown"}
          </span>
        </div>
        <div className="panel">
          <div className="mirror-body">
            <div className="mirror-status">
              {!status ? (
                <span className="muted">Reading mirror status…</span>
              ) : (
                <>
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
              <span className="nav-keys">
                {(
                  [
                    ["back", "Back"],
                    ["home", "Home"],
                    ["recents", "Recents"],
                  ] as const
                ).map(([key, label]) => (
                  <button
                    key={key}
                    type="button"
                    className="btn"
                    disabled={status?.running !== true}
                    title={`Send the ${label} key to the device`}
                    onClick={() => sendKey(key)}
                  >
                    {label}
                  </button>
                ))}
              </span>
            </div>
          </div>

          {status?.running === true && (status.pictureBlankSecs ?? 0) >= 2 && (
            <p className="notice" role="status">
              <strong>Nothing to display.</strong> The stream is live and frames
              are arriving, but the picture has carried no visible content for{" "}
              {status.pictureBlankSecs} seconds. Protected content (DRM video) and
              a display that is off both look like this, and HermesGate cannot
              tell them apart — so it reports what it measured instead of
              claiming a cause.
            </p>
          )}

          {status?.running === true && status.stats ? (
            <>
              <div className="stats">
                <Stat label="Decode FPS" value={status.stats.fps.toFixed(1)} />
                <Stat
                  label="Received"
                  value={`${((status.stats.bytesPerS * 8) / 1e6).toFixed(1)} Mbit/s`}
                />
                <Stat label="Decode" value={`${status.stats.decodeMs.toFixed(1)} ms/s`} />
                <Stat label="Present" value={`${status.stats.blitMs.toFixed(1)} ms/s`} />
                <Stat label="Overwritten" value={status.stats.overwritten} />
                <Stat label="Frames decoded" value={status.stats.framesDecoded} />
              </div>
              <p className="muted mirror-hint">
                Pipeline stages measured on this machine: decode FPS and frames
                decoded are decoder output, Received is the socket, Decode and
                Present are the milliseconds each stage spent in the last second,
                Overwritten counts frames a newer frame replaced before they were
                painted. These are not input-to-mirror latency (that needs input
                injection, Phase 3). The optional CSV dump
                (<code>HERMESGATE_VIDEO_STATS</code>) records the same counters.
              </p>
              {status.audio && (
                <p className="muted mirror-hint">
                  {status.audio.error
                    ? `Audio is off: ${status.audio.error}. The picture is unaffected.`
                    : `Sound is the device's own playback (${status.audio.sampleRate} Hz ${
                        status.audio.channels === 2 ? "stereo" : "mono"
                      } PCM) sent to ${
                        status.audio.device || "the default output device"
                      }, which has played ${(status.audio.playedMs / 1000).toFixed(0)} s of it. The level is the loudest sample in the last second: ${
                        status.audio.peak === 0
                          ? "silence on the device"
                          : `${status.audio.peak}/32767`
                      }.`}
                </p>
              )}
            </>
          ) : (
            <div className="disc-body">
              <p className="muted">
                {status?.running === true
                  ? "Waiting for the first one-second sample…"
                  : "No session running — counters appear once mirroring starts."}
              </p>
            </div>
          )}
        </div>
      </section>

      <section className="section">
        <div className="section-label">
          <h2>Active Discovery</h2>
          <span className="section-note">
            {adb.error ? "degraded" : adb.available ? `adb · ${adb.source ?? "ready"}` : "adb unavailable"}
          </span>
        </div>
        <div className="panel">
          <div className="disc-body">
            <p>
              {adb.available && !adb.error
                ? "Scanning for device changes…"
                : "Discovery is not running."}
            </p>
            <p className="muted">
              The native watcher polls <code>adb devices</code> every 2 seconds and
              broadcasts a fresh snapshot whenever something changes. One phone
              reachable several ways (USB, its mDNS alias, Wi-Fi) is reported as
              one device with every transport it is on.
            </p>
          </div>
        </div>
      </section>

      {awaiting.length > 0 && (
        <section className="section">
          <div className="section-label">
            <h2>Awaiting Authorization</h2>
            <span className="section-note">{awaiting.length} pending</span>
          </div>
          <div className="panel">
            {awaiting.map((device) => (
              <div className="auth-item" key={device.serial}>
                <div>
                  <h4>{device.model ?? device.serial}</h4>
                  <p className="muted">
                    {device.serial} · confirm USB debugging on the device screen.
                  </p>
                </div>
                <span className="state">{device.state}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      <footer className="status-strip">
        <span>
          Service: <strong>{adb.error ? "degraded" : "Ready"}</strong>
        </span>
        <span>
          ADB: <strong>{adb.available ? (adb.source ?? "ready") : "unavailable"}</strong>
        </span>
        <span>
          Watcher: <strong>2s poll</strong>
        </span>
        <span>
          Devices: <strong>{list.length} known</strong>
          {offline > 0 && ` (${offline} offline)`}
        </span>
        <span>
          Mirror:{" "}
          <strong>{status ? (status.running ? `${status.phase} · ${sessionSerial}` : status.phase) : "unknown"}</strong>
        </span>
      </footer>
    </>
  );
}

/* ---------- Icons (inline SVG: no icon dependency, no network asset) ---------- */

function IconGauge() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M4 13h6V4H4zM14 20h6v-9h-6zM4 20h6v-4H4zM14 8h6V4h-6z" />
    </svg>
  );
}

function IconPhone({ size = 18 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="6" y="2.5" width="12" height="19" rx="2.5" />
      <path d="M10.5 18.5h3" />
    </svg>
  );
}

function IconSliders() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M4 7h8M17 7h3M4 17h3M12 17h8" />
      <circle cx="14.5" cy="7" r="2.2" />
      <circle cx="9.5" cy="17" r="2.2" />
    </svg>
  );
}

export default App;
