import { useEffect, useRef } from "react";

import { createSourceEditor } from "../source/createSourceEditor";
import type { MarkdownSession } from "../session/MarkdownSession";

export function SourceDocumentEditor({
  session,
  readOnly = false,
}: Readonly<{ session: MarkdownSession; readOnly?: boolean }>) {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!host.current) return;
    const editor = createSourceEditor(host.current, session, readOnly);
    return () => editor.destroy();
  }, [readOnly, session]);

  return <div className="source-editor" ref={host} aria-label="Markdown source editor" />;
}
