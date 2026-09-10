import { editorViewCtx, type Editor } from "@milkdown/kit/core";
import {
  createCodeBlockCommand,
  toggleEmphasisCommand,
  toggleInlineCodeCommand,
  toggleLinkCommand,
  toggleStrongCommand,
  wrapInBlockquoteCommand,
  wrapInBulletListCommand,
  wrapInHeadingCommand,
  wrapInOrderedListCommand,
} from "@milkdown/kit/preset/commonmark";
import { insertTableCommand, toggleStrikethroughCommand } from "@milkdown/kit/preset/gfm";
import { callCommand } from "@milkdown/kit/utils";
import { useEffect, useRef, useState } from "react";

import { createVisualSegmentEditor } from "../visual/createVisualSegmentEditor";
import type { VisualSession } from "../visual/VisualSession";
import { FormattingToolbar, type FormatCommand } from "../formatting/FormattingToolbar";

type VisualDocumentEditorProps = Readonly<{
  visual: VisualSession;
  onRequestSource(): void;
}>;

function VisualSegment({
  visual,
  segmentId,
  onFocus,
  onError,
}: Readonly<{
  visual: VisualSession;
  segmentId: string;
  onFocus(editor: Editor): void;
  onError(message: string): void;
}>) {
  const host = useRef<HTMLDivElement>(null);
  const editorRef = useRef<Editor | null>(null);

  useEffect(() => {
    let disposed = false;
    let editor: Awaited<ReturnType<typeof createVisualSegmentEditor>> | null = null;
    if (host.current) {
      void createVisualSegmentEditor(host.current, visual, segmentId).then(
        (created) => {
          if (disposed) void created.destroy();
          else {
            editor = created;
            editorRef.current = created.editor;
          }
        },
        (reason) => {
          if (!disposed) {
            onError(
              reason instanceof Error
                ? reason.message
                : "The visual editor could not open this Markdown block.",
            );
          }
        },
      );
    }
    return () => {
      disposed = true;
      editorRef.current = null;
      if (editor) void editor.destroy();
    };
  }, [onError, segmentId, visual]);

  return (
    <div
      className="visual-segment"
      ref={host}
      onFocus={() => editorRef.current && onFocus(editorRef.current)}
    />
  );
}

export function VisualDocumentEditor({ visual, onRequestSource }: VisualDocumentEditorProps) {
  const [activeEditor, setActiveEditor] = useState<Editor | null>(null);
  const [error, setError] = useState<string | null>(null);

  function run(command: FormatCommand) {
    const editor = activeEditor;
    if (!editor) return;
    switch (command) {
      case "heading1": editor.action(callCommand(wrapInHeadingCommand.key, 1)); break;
      case "heading2": editor.action(callCommand(wrapInHeadingCommand.key, 2)); break;
      case "bold": editor.action(callCommand(toggleStrongCommand.key)); break;
      case "italic": editor.action(callCommand(toggleEmphasisCommand.key)); break;
      case "strikethrough": editor.action(callCommand(toggleStrikethroughCommand.key)); break;
      case "link": {
        const href = globalThis.prompt("Link address", "https://");
        if (href) editor.action(callCommand(toggleLinkCommand.key, { href }));
        break;
      }
      case "bulletList": editor.action(callCommand(wrapInBulletListCommand.key)); break;
      case "orderedList": editor.action(callCommand(wrapInOrderedListCommand.key)); break;
      case "taskList": {
        editor.action(callCommand(wrapInBulletListCommand.key));
        editor.action((ctx) => {
          const view = ctx.get(editorViewCtx);
          const position = view.state.selection.$from;
          for (let depth = position.depth; depth > 0; depth -= 1) {
            const node = position.node(depth);
            if (node.type.name === "list_item") {
              view.dispatch(
                view.state.tr.setNodeMarkup(position.before(depth), undefined, {
                  ...node.attrs,
                  checked: false,
                }),
              );
              return true;
            }
          }
          return false;
        });
        break;
      }
      case "quote": editor.action(callCommand(wrapInBlockquoteCommand.key)); break;
      case "inlineCode": editor.action(callCommand(toggleInlineCodeCommand.key)); break;
      case "codeBlock": editor.action(callCommand(createCodeBlockCommand.key)); break;
      case "table": editor.action(callCommand(insertTableCommand.key, { row: 3, col: 3 })); break;
    }
  }

  return (
    <div className="visual-editor-shell">
      <FormattingToolbar disabled={activeEditor === null} onCommand={run} />
      {error && <div className="editor-message" role="alert">{error}</div>}
      <div className="visual-editor" aria-label="Visual Markdown editor">
        {visual.projection.segments.map((segment) =>
          segment.kind === "visual" ? (
            <VisualSegment
              key={segment.id}
              visual={visual}
              segmentId={segment.id}
              onFocus={setActiveEditor}
              onError={setError}
            />
          ) : (
            <div className="source-backed-block" key={segment.id}>
              <div><span>Source-only Markdown</span><button type="button" onClick={onRequestSource}>Edit in Source</button></div>
              <pre>{segment.source}</pre>
            </div>
          ),
        )}
      </div>
    </div>
  );
}
