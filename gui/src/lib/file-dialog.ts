import { open, save } from "@tauri-apps/plugin-dialog";

export async function pickOpenFile(title: string, defaultPath?: string | null): Promise<string | null> {
  const path = await open({
    title,
    multiple: false,
    directory: false,
    defaultPath: defaultPath ?? undefined,
  });
  return path ?? null;
}

export async function pickSaveFile(title: string, defaultPath?: string | null): Promise<string | null> {
  const path = await save({
    title,
    defaultPath: defaultPath ?? undefined,
  });
  return path ?? null;
}
