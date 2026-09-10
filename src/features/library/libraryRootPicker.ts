import { open } from "@tauri-apps/plugin-dialog";

import {
  bindLibraryRoot,
  probeLibraryRoot,
  type LibrarySnapshot,
} from "../../lib/tauri/library";

export async function chooseLibraryRoot(): Promise<string | null> {
  return open({
    title: "Choose your Cairn.md library folder",
    directory: true,
    multiple: false,
  });
}

export async function chooseAndBindLibraryRoot(): Promise<LibrarySnapshot | null> {
  const selected = await chooseLibraryRoot();
  if (selected === null) {
    return null;
  }
  const probe = await probeLibraryRoot(selected);
  if (!probe.canBind) {
    throw new Error(probe.reason ?? "This folder cannot be used as a Cairn.md library.");
  }
  return bindLibraryRoot(selected);
}
