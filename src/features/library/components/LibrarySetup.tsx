import { useState } from "react";

import type { LibrarySnapshot } from "../../../lib/tauri/library";
import { chooseAndBindLibraryRoot } from "../libraryRootPicker";

type LibrarySetupProps = Readonly<{
  onBound(snapshot: LibrarySnapshot): void;
}>;

export function LibrarySetup({ onBound }: LibrarySetupProps) {
  const [error, setError] = useState<string | null>(null);
  const [choosing, setChoosing] = useState(false);

  async function choose() {
    setChoosing(true);
    setError(null);
    try {
      const snapshot = await chooseAndBindLibraryRoot();
      if (snapshot) onBound(snapshot);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The library folder could not be opened.");
    } finally {
      setChoosing(false);
    }
  }

  return (
    <main className="setup-screen">
      <section className="setup-card" aria-labelledby="setup-heading">
        <span className="setup-mark" aria-hidden="true">C</span>
        <p className="eyebrow">Your local Markdown home</p>
        <h1 id="setup-heading">Choose a folder for your Cairn.md library.</h1>
        <p>
          Projects and portable Markdown files live here. Cairn.md keeps its private
          organization metadata separately, so your documents stay clean and shareable.
        </p>
        <button className="primary-button" type="button" disabled={choosing} onClick={() => void choose()}>
          {choosing ? "Checking folder…" : "Choose library folder"}
        </button>
        {error && <p className="inline-error" role="alert">{error}</p>}
      </section>
    </main>
  );
}
