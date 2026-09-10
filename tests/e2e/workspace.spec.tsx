import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Workspace } from "../../src/features/library/components/Workspace";
import type { LibrarySnapshot } from "../../src/lib/tauri/library";

vi.mock("../../src/features/library/tracked-folders/trackedFolderBrowser", () => ({
  addChosenTrackedFolder: vi.fn(),
  browseTrackedFolder: vi.fn(async () => []),
  importTrackedFile: vi.fn(),
  loadTrackedFolders: vi.fn(async () => []),
}));

const snapshot: LibrarySnapshot = {
  mode: "writable",
  readOnlyReason: null,
  binding: {
    libraryId: "library-1",
    rootPath: "C:\\Notes",
    rootIdentity: "root-1",
    generation: 1,
  },
  projects: [
    {
      id: "project-1",
      relativePath: "Alpha",
      documents: [
        {
          id: "document-1",
          relativePath: "Alpha/brief.md",
          sourcePath: null,
          importedAt: null,
          diskFingerprint: "sha256:abc",
        },
      ],
    },
  ],
};

describe("three-pane workspace", () => {
  it("keeps the active document visible when contents are collapsed and restored", () => {
    render(
      <Workspace
        initialSnapshot={snapshot}
        initialTrackedFolders={[]}
        appearance="followWindows"
        onAppearanceChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Library")).toBeInTheDocument();
    expect(screen.getByLabelText("Project contents")).toBeInTheDocument();
    expect(screen.getByLabelText("Document editor")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("option", { name: /brief\.md/i }));
    expect(screen.getByRole("heading", { name: "brief.md" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Hide project contents" }));
    expect(screen.queryByLabelText("Project contents")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "brief.md" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show contents" }));
    expect(screen.getByLabelText("Project contents")).toBeInTheDocument();
  });

  it("exposes only the approved appearance choices", () => {
    render(
      <Workspace
        initialSnapshot={snapshot}
        initialTrackedFolders={[]}
        appearance="followWindows"
        onAppearanceChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("combobox", { name: "Appearance" })).toHaveTextContent(
      "Follow WindowsLightDark",
    );
    expect(screen.queryByText(/preview|split/i)).not.toBeInTheDocument();
  });
});
