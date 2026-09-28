import React, { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Outlet, useNavigate } from "@tanstack/react-router";
import { Sidebar } from "./Sidebar";
import { RecoveryModal } from "../ui/RecoveryModal";
import { useRecordingStore } from "../../stores/recordingStore";
import { useUiStore } from "../../stores/uiStore";
import { useWindowResync } from "../../lib/useWindowResync";
import { showRecordingWindow } from "../../lib/tauri";

export const AppShell: React.FC = () => {
  const navigate = useNavigate();
  const { recoverable, state: recordingState, tick } = useRecordingStore();
  const { activeRecoveryModal, setActiveRecoveryModal } = useUiStore();
  const [backgroundNotice, setBackgroundNotice] = useState(false);

  // Resynchronize on window focus/visibility
  useWindowResync();

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen("capture-backgrounded", () => setBackgroundNotice(true))
      .then((dispose) => { unlisten = dispose; })
      .catch(() => undefined);
    return () => unlisten?.();
  }, []);

  // Recording elapsed timer interval
  useEffect(() => {
    let interval: NodeJS.Timeout | null = null;
    if (recordingState === "recording") {
      interval = setInterval(() => {
        tick();
      }, 1000);
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [recordingState, tick]);

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
        <div className="fixed top-3 left-1/2 z-50 -translate-x-1/2 flex items-center gap-3 rounded-lg border border-border-active bg-surface-elevated px-4 py-2 text-xs shadow-lg" role="status">
          <span>Capture is still running in the background.</span>
          <button type="button" className="font-semibold text-green-text hover:underline" onClick={() => { setBackgroundNotice(false); void showRecordingWindow(); }}>
            Reopen controls
          </button>
        </div>
      )}
      {/* 240px Persistent Sidebar (collapsible to 60px) */}
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
