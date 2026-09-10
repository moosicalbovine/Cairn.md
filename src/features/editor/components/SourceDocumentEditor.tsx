import { useEffect, useRef } from "react";

import { createSourceEditor } from "../source/createSourceEditor";
import type { MarkdownSession } from "../session/MarkdownSession";

export function SourceDocumentEditor({ session }: Readonly<{ session: MarkdownSession }>) {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!host.current) return;
    const editor = createSourceEditor(host.current, session);
    return () => editor.destroy();
  }, [session]);

  return <div className="source-editor" ref={host} aria-label="Markdown source editor" />;
}
