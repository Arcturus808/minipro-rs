import { writable, derived } from "svelte/store";

export interface LogEntry {
  timestamp: Date;
  level: "info" | "warn" | "error";
  message: string;
}

function createLogStore() {
  const { subscribe, update } = writable<LogEntry[]>([]);

  return {
    subscribe,
    add: (level: LogEntry["level"], message: string) => {
      update((logs) => {
        const entry: LogEntry = { timestamp: new Date(), level, message };
        // Keep last 2000 entries to prevent memory bloat
        const next = [...logs, entry];
        if (next.length > 2000) next.splice(0, next.length - 2000);
        return next;
      });
    },
    clear: () => update(() => []),
    info: (message: string) => {
      update((logs) => {
        const entry: LogEntry = { timestamp: new Date(), level: "info", message };
        const next = [...logs, entry];
        if (next.length > 2000) next.splice(0, next.length - 2000);
        return next;
      });
    },
    warn: (message: string) => {
      update((logs) => {
        const entry: LogEntry = { timestamp: new Date(), level: "warn", message };
        const next = [...logs, entry];
        if (next.length > 2000) next.splice(0, next.length - 2000);
        return next;
      });
    },
    error: (message: string) => {
      update((logs) => {
        const entry: LogEntry = { timestamp: new Date(), level: "error", message };
        const next = [...logs, entry];
        if (next.length > 2000) next.splice(0, next.length - 2000);
        return next;
      });
    },
  };
}

export const logs = createLogStore();

export const logText = derived(logs, ($logs) =>
  $logs
    .map(
      (entry) =>
        `[${entry.timestamp.toLocaleTimeString()}] [${entry.level.toUpperCase()}] ${entry.message}`,
    )
    .join("\n"),
);
