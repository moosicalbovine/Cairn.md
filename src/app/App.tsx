import { useEffect, useState } from "react";

import { getHealth } from "../lib/tauri/health";
import "./app.css";

type ConnectionState = "checking" | "ready" | "unavailable";

export function App() {
  const [connection, setConnection] = useState<ConnectionState>("checking");

  useEffect(() => {
    let active = true;

    void getHealth().then(
      () => active && setConnection("ready"),
      () => active && setConnection("unavailable"),
    );

    return () => {
      active = false;
    };
  }, []);

  return (
    <main className="app-shell">
      <header className="titlebar">
        <div className="brand-mark" aria-hidden="true">
          N
        </div>
        <div>
          <p className="eyebrow">Local Markdown workspace</p>
          <h1>Cairn.md</h1>
        </div>
      </header>

      <section className="foundation-card" aria-labelledby="foundation-heading">
        <p className="status-pill" data-state={connection}>
          {connection === "checking" && "Starting locally…"}
          {connection === "ready" && "Desktop core ready"}
          {connection === "unavailable" && "Open with the Cairn.md desktop app"}
        </p>
        <h2 id="foundation-heading">Your Markdown library is taking shape.</h2>
        <p>
          Cairn.md runs on your PC and keeps portable Markdown files under your
          control. Library setup arrives in the next implementation unit.
        </p>
      </section>
    </main>
  );
}
