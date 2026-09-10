import { markdown } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { basicSetup, EditorView } from "codemirror";

import { MarkdownSession } from "../session/MarkdownSession";

export interface SourceEditorHandle {
  readonly view: EditorView;
  destroy(): void;
}

export function createSourceEditor(
  parent: HTMLElement,
  session: MarkdownSession,
  readOnly = false,
): SourceEditorHandle {
  let applyingSessionChange = false;

  const view = new EditorView({
    parent,
    doc: session.source,
    extensions: [
      basicSetup,
      markdown(),
      EditorState.lineSeparator.of(
        session.lineEnding === "crlf" ? "\r\n" : "\n",
      ),
      EditorView.editable.of(!session.isReadOnly && !readOnly),
      EditorView.updateListener.of((update) => {
        if (!update.docChanged || applyingSessionChange) {
          return;
        }
        session.replaceSource(update.state.doc.toString(), session.revision);
      }),
    ],
  });

  const unsubscribe = session.subscribe(({ source }) => {
    if (view.state.doc.toString() === source) {
      return;
    }

    applyingSessionChange = true;
    try {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: source },
      });
    } finally {
      applyingSessionChange = false;
    }
  });

  return {
    view,
    destroy() {
      unsubscribe();
      view.destroy();
    },
  };
}
