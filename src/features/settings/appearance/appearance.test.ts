import { describe, expect, it, vi } from "vitest";

import { applyAppearance, loadAppearance, storeAppearance } from "./appearance";

describe("appearance preference", () => {
  it("defaults to Follow Windows and applies explicit themes only", () => {
    const storage = { getItem: vi.fn(() => null) };
    const root = document.createElement("html");

    expect(loadAppearance(storage)).toBe("followWindows");
    applyAppearance(root, "dark");
    expect(root.dataset.theme).toBe("dark");
    applyAppearance(root, "followWindows");
    expect(root).not.toHaveAttribute("data-theme");
  });

  it("persists only the selected appearance value", () => {
    const storage = { setItem: vi.fn() };

    storeAppearance(storage, "light");

    expect(storage.setItem).toHaveBeenCalledWith("cairn.appearance", "light");
  });
});
