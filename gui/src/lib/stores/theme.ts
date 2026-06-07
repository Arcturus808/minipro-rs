import { writable } from "svelte/store";

export type Theme = "system" | "dark" | "light";

function createThemeStore() {
  const { subscribe, set } = writable<Theme>("system");

  function apply(theme: Theme) {
    const html = document.documentElement;
    html.setAttribute("data-theme", theme);
    // Also sync class for Tailwind dark mode
    const isDark =
      theme === "dark" ||
      (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
    if (isDark) {
      html.classList.add("dark");
    } else {
      html.classList.remove("dark");
    }
  }

  return {
    subscribe,
    set: (theme: Theme) => {
      apply(theme);
      set(theme);
    },
    init: () => {
      const saved = localStorage.getItem("theme") as Theme | null;
      const initial = saved ?? "system";
      apply(initial);
      set(initial);

      window
        .matchMedia("(prefers-color-scheme: dark)")
        .addEventListener("change", () => {
          const current = document.documentElement.getAttribute("data-theme") as Theme;
          if (current === "system") {
            apply("system");
          }
        });
    },
  };
}

export const theme = createThemeStore();
