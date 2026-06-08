import { open, save } from "@tauri-apps/plugin-dialog";

export async function pickOpenFile(title: string): Promise<string | null> {
  const path = await open({
    title,
    multiple: false,
    directory: false,
    filters: [
      { name: "All Supported", extensions: ["bin", "hex", "srec", "mot", "jed"] },
      { name: "Binary", extensions: ["bin"] },
      { name: "Intel HEX", extensions: ["hex"] },
      { name: "SREC", extensions: ["srec", "mot"] },
      { name: "JEDEC", extensions: ["jed"] },
    ],
  });
  return path ?? null;
}

export async function pickSaveFile(title: string): Promise<string | null> {
  const path = await save({
    title,
    filters: [
      { name: "Binary", extensions: ["bin"] },
      { name: "Intel HEX", extensions: ["hex"] },
      { name: "SREC", extensions: ["srec", "mot"] },
      { name: "JEDEC", extensions: ["jed"] },
    ],
  });
  return path ?? null;
}
