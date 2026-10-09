import React, { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Outlet, useNavigate } from "@tanstack/react-router";
import { Sidebar } from "./Sidebar";
import { RecoveryModal } from "../ui/RecoveryModal";
import { useRecordingStore } from "../../stores/recordingStore";
import { useUiStore } from "../../stores/uiStore";
import { useWindowResync } from "../../lib/useWindowResync";
import { useInputModality } from "../../lib/useInputModality";
import { showRecordingWindow, isTauriEnvironment } from "../../lib/tauri";

export const AppShell: React.FC = () => {
  const navigate = useNavigate();
  const { recoverable, state: recordingState, tick, syncFromBackend } = useRecordingStore();
  const { activeRecoveryModal, setActiveRecoveryModal } = useUiStore();
  const [backgroundNotice, setBackgroundNotice] = useState(false);

  // Resynchronize on window focus/visibility
  useWindowResync();

  // Pointer use clears focus highlights on pills/buttons; keyboard use restores them
  useInputModality();

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen("capture-backgrounded", () => setBackgroundNotice(true))
      .then((dispose) => { unlisten = dispose; })
      .catch(() => undefined);
    return () => unlisten?.();
  }, []);

  // Live recording HUD: in the desktop app the Rust capture engine is the single
  // source of truth for elapsed time and dBFS levels, so poll it. Outside Tauri
  // (browser/mock mode) fall back to a local 1s tick.
  useEffect(() => {
    const live = isTauriEnvironment();
    const active = recordingState === "recording" || (live && recordingState === "paused");
    if (!active) return;
    const interval = setInterval(
      () => {
        if (live) void syncFromBackend();
        else tick();
      },
      live ? 200 : 1000
    );
    return () => clearInterval(interval);
  }, [recordingState, tick, syncFromBackend]);

  // Prompt recovery modal on launch if an interrupted session is detected
  useEffect(() => {
    if (recoverable) {
      setActiveRecoveryModal(true);
    }
  }, [recoverable, setActiveRecoveryModal]);

  // Global keyboard shortcuts (DESIGN.md §6.2)
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const isMetaOrCtrl = e.metaKey || e.ctrlKey;

      if (isMetaOrCtrl && (e.key === "n" || e.key === "r")) {
        e.preventDefault();
        void navigate({ to: "/record" });
      } else if (isMetaOrCtrl && e.key === "b") {
        e.preventDefault();
        useUiStore.getState().toggleSidebar();
      } else if (isMetaOrCtrl && e.key === ",") {
        e.preventDefault();
        void navigate({ to: "/settings" });
      } else if (isMetaOrCtrl && e.key === "k") {
        e.preventDefault();
        void navigate({ to: "/" });
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [navigate]);

  return (
    <div className="min-h-screen flex bg-bg text-ink font-sans antialiased">
      {backgroundNotice && recordingState !== "idle" && (
        <div className="fixed top-3 left-1/2 z-50 -translate-x-1/2 flex items-center gap-3 rounded-full border border-border bg-surface-elevated px-4 py-2 text-xs shadow-lg" role="status">
          <span>Capture is still running in the background.</span>
          <button type="button" className="font-semibold text-green-text hover:underline" onClick={() => { setBackgroundNotice(false); void showRecordingWindow(); }}>
            Reopen controls
          </button>
        </div>
      )}
      {/* Floating inset sidebar (collapses to an icon rail) */}
      <Sidebar />

      {/* Main Workspace Area */}
      <main className="flex-1 flex flex-col min-w-0 bg-bg overflow-x-hidden">
        <Outlet />
      </main>

      {/* Crash Recovery Modal */}
      <RecoveryModal
        isOpen={activeRecoveryModal}
        onClose={() => setActiveRecoveryModal(false)}
        onRecover={() => {
          setActiveRecoveryModal(false);
          void navigate({ to: "/" });
        }}
        onDiscard={() => {
          setActiveRecoveryModal(false);
        }}
      />
    </div>
  );
};
