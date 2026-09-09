import { Editor, defaultValueCtx, rootCtx } from "@milkdown/kit/core";
import { listener, listenerCtx } from "@milkdown/kit/plugin/listener";
import { commonmark } from "@milkdown/kit/preset/commonmark";
import { gfm } from "@milkdown/kit/preset/gfm";

import { VisualSession } from "./VisualSession";

export interface VisualSegmentEditorHandle {
  readonly editor: Editor;
  destroy(): Promise<void>;
}

export async function createVisualSegmentEditor(
  parent: HTMLElement,
  visual: VisualSession,
  segmentId: string,
): Promise<VisualSegmentEditorHandle> {
  const initial = visual.projection.segments.find(
    (segment) => segment.id === segmentId,
  );
  if (!initial || initial.kind !== "visual") {
    throw new Error("A Milkdown editor can only mount a visual Markdown segment.");
  }

  let activeSegmentId = initial.id;
  let activeRevision = visual.projection.revision;
  let activeFrom = initial.from;
  let lastMarkdown = initial.source;

  const editor = await Editor.make()
    .config((ctx) => {
      ctx.set(rootCtx, parent);
      ctx.set(defaultValueCtx, initial.source);
      ctx.get(listenerCtx).markdownUpdated((_ctx, markdown) => {
        if (markdown === lastMarkdown) {
          return;
        }

        visual.replaceSegment(activeSegmentId, markdown, activeRevision);
        const replacement = visual.projection.segments.find(
          (segment) =>
            segment.kind === "visual" &&
            segment.from === activeFrom &&
            segment.source === markdown,
        );
        if (!replacement) {
          throw new Error("Visual source mapping could not be rebuilt after edit.");
        }

        activeSegmentId = replacement.id;
        activeRevision = visual.projection.revision;
        activeFrom = replacement.from;
        lastMarkdown = markdown;
      });
    })
    .use(commonmark)
    .use(gfm)
    .use(listener)
    .create();

  return {
    editor,
    async destroy() {
      await editor.destroy();
    },
  };
}
