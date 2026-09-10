import {
  importMarkdown,
  type ImportSource,
} from "../../../lib/tauri/import";
import type { DocumentSnapshot } from "../../../lib/tauri/library";

export type ImportedDocumentTarget = Readonly<{
  projectId: string;
  documentId: string;
  editorMode: "visual";
}>;

export type ImportSuccess = Readonly<{
  document: DocumentSnapshot;
  navigation: ImportedDocumentTarget;
}>;

export type Importer = (
  projectId: string,
  source: ImportSource,
) => Promise<DocumentSnapshot>;

export class ImportQueue {
  private tail: Promise<void> = Promise.resolve();

  public constructor(private readonly importer: Importer = importMarkdown) {}

  public enqueue(projectId: string, source: ImportSource): Promise<ImportSuccess> {
    const run = this.tail.then(async () => {
      const document = await this.importer(projectId, source);
      return {
        document,
        navigation: {
          projectId,
          documentId: document.id,
          editorMode: "visual" as const,
        },
      };
    });
    this.tail = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }
}
