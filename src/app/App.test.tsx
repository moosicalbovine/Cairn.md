import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getHealth } from "../lib/tauri/health";
import { isPerformanceMode } from "../lib/tauri/performance";
import { App } from "./App";

vi.mock("../lib/tauri/health", () => ({ getHealth: vi.fn() }));
vi.mock("../lib/tauri/performance", () => ({ isPerformanceMode: vi.fn() }));
vi.mock("../lib/tauri/library", () => ({ loadCachedLibraryIndex: vi.fn() }));
vi.mock("../lib/tauri/import", () => ({ listTrackedFolders: vi.fn() }));

describe("App startup failure", () => {
  beforeEach(() => {
    vi.mocked(isPerformanceMode).mockResolvedValue(false);
  });

  it("explains the failure and gives restart guidance without diagnostic detail", async () => {
    vi.mocked(getHealth).mockRejectedValue("desktop unavailable");

    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "Cairn.md could not finish starting" }),
    ).toBeInTheDocument();
    expect(screen.getByText("No diagnostic details were provided.")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Close this window, then start or restart the installed Cairn.md Windows application.",
      ),
    ).toBeInTheDocument();
  });

  it("keeps diagnostic detail separate from the restart guidance", async () => {
    vi.mocked(getHealth).mockRejectedValue(new Error("Invalid health response"));

    render(<App />);

    expect(
      await screen.findByRole("heading", { name: "Cairn.md could not finish starting" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Invalid health response")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Close this window, then start or restart the installed Cairn.md Windows application.",
      ),
    ).toBeInTheDocument();
  });
});
