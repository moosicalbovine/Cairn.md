export type Appearance = "followWindows" | "light" | "dark";

const STORAGE_KEY = "cairn.appearance";

export function loadAppearance(storage: Pick<Storage, "getItem">): Appearance {
  const value = storage.getItem(STORAGE_KEY);
  return value === "light" || value === "dark" || value === "followWindows"
    ? value
    : "followWindows";
}

export function storeAppearance(
  storage: Pick<Storage, "setItem">,
  appearance: Appearance,
): void {
  storage.setItem(STORAGE_KEY, appearance);
}

export function applyAppearance(root: HTMLElement, appearance: Appearance): void {
  if (appearance === "followWindows") {
    root.removeAttribute("data-theme");
  } else {
    root.dataset.theme = appearance;
  }
}
