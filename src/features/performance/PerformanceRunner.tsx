import { editorViewCtx } from "@milkdown/kit/core";
import { useEffect, useRef, useState } from "react";

import {
  getPerformanceScenario,
  markPerformanceReady,
  writePerformanceReport,
} from "../../lib/tauri/performance";
import { MarkdownSession } from "../editor/session/MarkdownSession";
import { VisualSession } from "../editor/visual/VisualSession";
import {
  createVisualSegmentEditor,
  type VisualSegmentEditorHandle,
} from "../editor/visual/createVisualSegmentEditor";

const sampleCount = 20;

function normalDocument(): string {
  return Array.from(
    { length: 80 },
    (_, index) =>
      `## Section ${index + 1}\n\nParagraph ${index + 1} contains **portable Markdown**, a [link](https://example.com), and enough text for a representative document.`,
  ).join("\n\n");
}

async function afterPaint(): Promise<void> {
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });
}

async function openVisualEditor(parent: HTMLElement, source: string): Promise<{
  editor: VisualSegmentEditorHandle;
  visual: VisualSession;
}> {
  const session = MarkdownSession.fromSource(source);
  const visual = new VisualSession(session);
  const segment = visual.projection.segments.find((candidate) => candidate.kind === "visual");
  if (!segment) throw new Error("The performance fixture has no visual segment");
  const editor = await createVisualSegmentEditor(parent, visual, segment.id);
  return { editor, visual };
}

export function PerformanceRunner() {
  const primaryHost = useRef<HTMLDivElement>(null);
  const scratchHost = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("Preparing release measurements…");

  useEffect(() => {
    const primaryParent = primaryHost.current;
    const scratchParent = scratchHost.current;
    if (!primaryParent || !scratchParent) return;
    let active = true;
    let primary: Awaited<ReturnType<typeof openVisualEditor>> | null = null;

    void (async () => {
      try {
        const scenario = await getPerformanceScenario();
        const source = normalDocument();
        primary = await openVisualEditor(primaryParent, source);
        await afterPaint();
        if (!active) return;
        await markPerformanceReady();
        if (scenario === "idle") {
          setStatus("Cairn.md is idle with one document open.");
          return;
        }

        setStatus("Measuring visual input latency…");
        const inputLatencyMs: number[] = [];
        for (let sample = 0; sample < sampleCount; sample += 1) {
          const startedAt = performance.now();
          primary.editor.editor.action((ctx) => {
            const view = ctx.get(editorViewCtx);
            view.dispatch(view.state.tr.insertText("x"));
          });
          await afterPaint();
          inputLatencyMs.push(Number((performance.now() - startedAt).toFixed(3)));
        }

        setStatus("Measuring normal document open time…");
        const documentOpenMs: number[] = [];
        for (let sample = 0; sample < sampleCount; sample += 1) {
          const host = document.createElement("div");
          scratchParent.replaceChildren(host);
          const startedAt = performance.now();
          const opened = await openVisualEditor(host, source);
          await afterPaint();
          documentOpenMs.push(Number((performance.now() - startedAt).toFixed(3)));
          await opened.editor.destroy();
          opened.visual.dispose();
        }
        scratchParent.replaceChildren();

        if (!active) return;
        await writePerformanceReport({ documentOpenMs, inputLatencyMs });
        if (active) setStatus("Measurements complete. Cairn.md is idle.");
      } catch (reason) {
        if (active) {
          setStatus(
            reason instanceof Error ? reason.message : "Performance measurement failed.",
          );
        }
      }
    })();

    return () => {
      active = false;
      if (primary) {
        void primary.editor.destroy();
        primary.visual.dispose();
      }
    };
  }, []);

  return (
    <main className="performance-runner">
      <p>{status}</p>
      <div className="performance-editor" ref={primaryHost} />
      <div className="performance-scratch" ref={scratchHost} aria-hidden="true" />
    </main>
  );
}
