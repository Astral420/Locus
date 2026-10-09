import { isTauriEnvironment } from "./tauri";

/** True on macOS, where shortcut hints use the command symbol. */
export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent || "");
}

/** True when the window uses the macOS overlay title bar, so the app must leave room for the traffic lights. */
export function hasOverlayTitleBar(): boolean {
  return isMacPlatform() && isTauriEnvironment();
}

/**
 * Left padding class for a page's top row while the sidebar is hidden: clears the traffic lights (macOS overlay)
 * and the reopen/download controls that float beside them.
 */
export function hiddenSidebarInset(sidebarHidden: boolean): string {
  if (!sidebarHidden) return "";
  return hasOverlayTitleBar() ? "pl-[164px]" : "pl-[92px]";
}
