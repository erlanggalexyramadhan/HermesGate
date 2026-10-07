import { useEffect, useState } from "react";
import { core, type AppInfo, type ModuleInfo } from "./lib/tauri";
import "./App.css";

const NAV: { id: string; label: string; ready: boolean }[] = [
  { id: "dashboard", label: "Dashboard", ready: true },
  { id: "devices", label: "Devices", ready: false },
  { id: "mirroring", label: "Mirroring", ready: false },
  { id: "settings", label: "Settings", ready: false },
];

type Load =
  | { status: "loading" }
  | { status: "ready"; app: AppInfo; modules: ModuleInfo[] }
  | { status: "error"; message: string };

function App() {
  const [load, setLoad] = useState<Load>({ status: "loading" });

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
              className={`nav-item${item.ready ? " is-active" : ""}`}
              disabled={!item.ready}
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
          <h1>Dashboard</h1>
          {load.status === "ready" && (
            <span className="chip">
              {load.app.platform} / {load.app.arch}
            </span>
          )}
        </header>

        <div className="content">
          {load.status === "loading" && (
            <p className="muted">Contacting the native core…</p>
          )}

          {load.status === "error" && (
            <div className="banner">
              Native bridge unavailable — this UI must run inside the
              HermesGate shell (<code>npm run tauri dev</code>).
              <div className="banner-detail">{load.message}</div>
            </div>
          )}

          {load.status === "ready" && (
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
        </div>
      </main>
    </div>
  );
}

export default App;
