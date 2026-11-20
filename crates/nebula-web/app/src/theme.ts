import { useSyncExternalStore } from "react";

export type Theme = "dark" | "light";

/// Where this device keeps its choice. index.html reads the same key
/// before the first paint, so the page never flashes the other theme.
const KEY = "nebula.theme";
const LIGHT = "(prefers-color-scheme: light)";

type Store = Pick<Storage, "getItem" | "setItem">;

/// The page's storage, or none where reaching it throws, as in a private
/// window or with site data blocked.
function store(): Store | null {
  try {
    return localStorage;
  } catch {
    return null;
  }
}

/// The theme this device chose, if it chose one.
export function chosen(s: Store | null): Theme | null {
  try {
    const t = s?.getItem(KEY);
    return t === "dark" || t === "light" ? t : null;
  } catch {
    return null;
  }
}

export function choose(s: Store | null, theme: Theme): void {
  try {
    s?.setItem(KEY, theme);
  } catch {
    // Not remembered past this page, then.
  }
}

/// The theme to show: this device's choice, else the system's.
export function themeOf(s: Store | null, systemLight: boolean): Theme {
  return chosen(s) ?? (systemLight ? "light" : "dark");
}

/// Puts `theme` on the page, and the colour the page's top now is in the
/// browser's own bar.
function apply(theme: Theme) {
  document.documentElement.dataset.theme = theme;
  requestAnimationFrame(() => {
    const top = getComputedStyle(document.body).backgroundColor;
    document.querySelector('meta[name="theme-color"]')?.setAttribute("content", top);
  });
}

/// Shows the theme due, and follows the system's as it changes while this
/// device has chosen none.
export function watchTheme() {
  const system = matchMedia(LIGHT);
  apply(themeOf(store(), system.matches));
  system.addEventListener("change", () => chosen(store()) === null && apply(system.matches ? "light" : "dark"));
}

/// Shows the other theme, and keeps it as this device's choice.
export function toggleTheme() {
  const next = currentTheme() === "dark" ? "light" : "dark";
  choose(store(), next);
  apply(next);
}

function currentTheme(): Theme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

function subscribe(changed: () => void) {
  const seen = new MutationObserver(changed);
  seen.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  return () => seen.disconnect();
}

export function useTheme(): Theme {
  return useSyncExternalStore(subscribe, currentTheme, () => "dark");
}
