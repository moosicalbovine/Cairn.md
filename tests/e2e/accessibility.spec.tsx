import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { WorkspaceLayout } from "../../src/components/layout/WorkspaceLayout";
import { LibraryPane } from "../../src/features/library/components/LibraryPane";
import type { ProjectSnapshot } from "../../src/lib/tauri/library";

vi.mock("../../src/features/library/tracked-folders/trackedFolderBrowser", () => ({
  browseTrackedFolder: vi.fn(async () => []),
}));

function LayoutHarness() {
  const [visible, setVisible] = useState(true);
  return (
    <WorkspaceLayout
      contentsVisible={visible}
      onContentsVisibleChange={setVisible}
      library={<span>Library navigation</span>}
      contents={(
        <>
          <button type="button" onClick={() => setVisible(false)}>Hide contents</button>
          <button type="button" role="option" aria-selected="true">Selected file</button>
        </>
      )}
      editor={<span>Editor surface</span>}
    />
  );
}

const projects: readonly ProjectSnapshot[] = [
  { id: "alpha", relativePath: "Alpha", documents: [] },
  { id: "beta", relativePath: "Beta", documents: [] },
];

describe("workspace accessibility", () => {
  it("labels all panes and supports keyboard resizing", () => {
    render(<LayoutHarness />);

    expect(screen.getByLabelText("Library")).toBeInTheDocument();
    expect(screen.getByLabelText("Project contents")).toBeInTheDocument();
    expect(screen.getByLabelText("Document editor")).toBeInTheDocument();

    const separator = screen.getByRole("separator", { name: "Resize project contents" });
    expect(separator).toHaveAttribute("aria-valuenow", "280");
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    expect(separator).toHaveAttribute("aria-valuenow", "296");
  });

  it("moves focus to the recovery control and back to the selected document", () => {
    render(<LayoutHarness />);

    fireEvent.click(screen.getByRole("button", { name: "Hide contents" }));
    expect(screen.getByRole("button", { name: "Show contents" })).toHaveFocus();

    fireEvent.click(screen.getByRole("button", { name: "Show contents" }));
    expect(screen.getByRole("option", { name: "Selected file" })).toHaveFocus();
  });

  it("moves through projects with arrow keys", () => {
    const onSelectProject = vi.fn();
    render(
      <LibraryPane
        projects={projects}
        selectedProjectId="alpha"
        trackedFolders={[]}
        canMutate={true}
        onSelectProject={onSelectProject}
        onCreateProject={vi.fn()}
        onRenameProject={vi.fn()}
        onAddTrackedFolder={vi.fn()}
        onRemoveTrackedFolder={vi.fn()}
        onImportTracked={vi.fn()}
      />,
    );

    const alpha = screen.getByRole("button", { name: "Alpha" });
    alpha.focus();
    fireEvent.keyDown(alpha, { key: "ArrowDown" });

    expect(onSelectProject).toHaveBeenCalledWith(projects[1]);
    expect(screen.getByRole("button", { name: "Beta" })).toHaveFocus();
  });
});
