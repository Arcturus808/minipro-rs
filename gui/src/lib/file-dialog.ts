import { invoke } from "@tauri-apps/api/core";
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

export async function pickSaveFile(title: string, defaultPath?: string | null, filters?: { name: string; extensions: string[] }[]): Promise<string | null> {
  const path = await save({
    title,
    defaultPath: defaultPath ?? undefined,
    filters: filters ?? undefined,
  });
  return path ?? null;
}

export async function fileExists(path: string): Promise<boolean> {
  return invoke<boolean>("file_exists", { path });
}

/**
 * Generate a unique save path by appending an incrementing number if the file already exists.
 *   "dump.bin"      → "dump.bin" (if not exists)
 *   "dump.bin"      → "dump (1).bin" (if exists)
 *   "dump (1).bin"  → "dump (2).bin" (if exists)
 */
export async function getIterativeSavePath(basePath: string): Promise<string> {
  if (!(await fileExists(basePath))) {
    return basePath;
  }
  const lastDot = basePath.lastIndexOf(".");
  const hasExtension = lastDot > basePath.lastIndexOf("\\") && lastDot > basePath.lastIndexOf("/");
  const stem = hasExtension ? basePath.slice(0, lastDot) : basePath;
  const ext = hasExtension ? basePath.slice(lastDot) : "";

  let counter = 1;
  let candidate = `${stem} (${counter})${ext}`;
  while (await fileExists(candidate)) {
    counter++;
    candidate = `${stem} (${counter})${ext}`;
  }
  return candidate;
}
