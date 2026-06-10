import { writable } from "svelte/store";
import { Store } from "@tauri-apps/plugin-store";

export interface AppSettings {
  defaultDirectory: string | null;
  defaultPage: "code" | "data" | "user";
  defaultFormat: "auto" | "bin" | "ihex" | "srec" | "jedec";
  defaultSizeMismatch: "error" | "warn" | "ignore";
  skipErase: boolean;
  skipVerify: boolean;
  theme: "system" | "dark" | "light";
  deviceViewMode: "paginated" | "scroll";
  hexViewerFontSize: number;
  leftPanelPercent: number;
  rightPanelPercent: number;
  windowWidth: number | null;
  windowHeight: number | null;
}

// Panel widths as fractions of window width (0.0–1.0)
// At 1280px: left=208px (16%), right=288px (22.5%)
const DEFAULTS: AppSettings = {
  defaultDirectory: null,
  defaultPage: "code",
  defaultFormat: "auto",
  defaultSizeMismatch: "error",
  skipErase: false,
  skipVerify: false,
  theme: "system",
  deviceViewMode: "paginated",
  hexViewerFontSize: 13,
  leftPanelPercent: 0.20,
  rightPanelPercent: 0.25,
  windowWidth: null,
  windowHeight: null,
};

let store: Store | null = null;

const settings = writable<AppSettings>({ ...DEFAULTS });

export async function initSettings() {
  store = await Store.load("settings.json");
  const loaded: Partial<AppSettings> = {};
  for (const key of Object.keys(DEFAULTS) as (keyof AppSettings)[]) {
    const val = await store.get<AppSettings[typeof key]>(key);
    if (val !== undefined && val !== null) {
      (loaded as any)[key] = val;
    }
  }
  settings.set({ ...DEFAULTS, ...loaded });
}

export async function setSetting<K extends keyof AppSettings>(
  key: K,
  value: AppSettings[K]
) {
  if (!store) return;
  await store.set(key, value);
  await store.save();
  settings.update((s) => ({ ...s, [key]: value }));
}

export { settings };
