import { lazy, Suspense, useEffect, useState } from "react";

import { LibrarySetup } from "../features/library/components/LibrarySetup";
import { Workspace } from "../features/library/components/Workspace";
import {
  applyAppearance,
  loadAppearance,
  storeAppearance,
  type Appearance,
} from "../features/settings/appearance/appearance";
import { listTrackedFolders, type TrackedFolderSnapshot } from "../lib/tauri/import";
import {
  loadCachedLibraryIndex,
  type LibrarySnapshot,
} from "../lib/tauri/library";
import { getHealth } from "../lib/tauri/health";
import { isPerformanceMode } from "../lib/tauri/performance";
import "./app.css";

const PerformanceRunner = lazy(() =>
  import("../features/performance/PerformanceRunner").then((module) => ({
    default: module.PerformanceRunner,
  })),
);

type BootState = "checking" | "performance" | "ready" | "unavailable";

export function App() {
  const [boot, setBoot] = useState<BootState>("checking");
  const [snapshot, setSnapshot] = useState<LibrarySnapshot | null>(null);
  const [trackedFolders, setTrackedFolders] = useState<readonly TrackedFolderSnapshot[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [appearance, setAppearance] = useState<Appearance>(() =>
    loadAppearance(globalThis.localStorage),
  );

  useEffect(() => {
    applyAppearance(document.documentElement, appearance);
    storeAppearance(globalThis.localStorage, appearance);
  }, [appearance]);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const [, performanceMode] = await Promise.all([
          getHealth(),
          isPerformanceMode(),
        ]);
        if (!active) return;
        if (performanceMode) {
          setBoot("performance");
          return;
        }
        const [library, tracked] = await Promise.all([
          loadCachedLibraryIndex((value) => {
            if (active) setSnapshot(value);
          }),
          listTrackedFolders(),
        ]);
        if (!active) return;
        setSnapshot(library);
        setTrackedFolders(tracked);
        setBoot("ready");
      } catch (reason) {
        if (!active) return;
        setError(reason instanceof Error ? reason.message : "The desktop core did not respond.");
        setBoot("unavailable");
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  if (boot === "checking") {
    return (
      <main className="startup-screen" aria-live="polite">
        <span className="setup-mark" aria-hidden="true">C</span>
        <p>Opening your Markdown library…</p>
      </main>
    );
  }

  if (boot === "performance") {
    return (
      <Suspense fallback={<main className="startup-screen">Loading performance profile…</main>}>
        <PerformanceRunner />
      </Suspense>
    );
  }

  if (boot === "unavailable" || snapshot === null) {
    return (
      <main className="startup-screen">
        <span className="setup-mark" aria-hidden="true">C</span>
        <h1>Open Cairn.md as a desktop app</h1>
        <p>{error ?? "The local desktop core is unavailable."}</p>
      </main>
    );
  }

  if (snapshot.binding === null) {
    return <LibrarySetup onBound={setSnapshot} />;
  }

  return (
    <Workspace
      key={`${snapshot.binding.libraryId}:${snapshot.binding.generation}`}
      initialSnapshot={snapshot}
      initialTrackedFolders={trackedFolders}
      appearance={appearance}
      onAppearanceChange={setAppearance}
    />
  );
}
