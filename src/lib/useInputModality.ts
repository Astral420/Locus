import { useEffect } from "react";

const INTERACTIVE = "input, textarea, select, button, a, summary, label, [role='button'], [role='switch'], [role='dialog'], [tabindex]";

/**
 * Tracks whether the user is driving the app with a pointer or the keyboard and exposes it as
 * `data-input-modality` on <html>. CSS hides focus highlights on pills/buttons/selects while the pointer is in use,
 * so a click anywhere clears them; pressing a key (Tab, arrows) brings the rings back for keyboard users.
 * A press on non-interactive space also blurs whatever still holds focus.
 */
export function useInputModality(): void {
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.inputModality = "pointer";

    const onPointerDown = (event: Event) => {
      root.dataset.inputModality = "pointer";
      const target = event.target as Element | null;
      if (target && !target.closest?.(INTERACTIVE)) {
        const active = document.activeElement as HTMLElement | null;
        if (active && active !== document.body) active.blur();
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      // Ignore bare modifiers so a shortcut chord alone doesn't flip the mode.
      if (["Shift", "Control", "Alt", "Meta"].includes(event.key)) return;
      root.dataset.inputModality = "keyboard";
    };

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("mousedown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("mousedown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
      delete root.dataset.inputModality;
    };
  }, []);
}
