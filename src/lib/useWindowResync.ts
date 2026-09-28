import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useRecordingStore } from "../stores/recordingStore";

export function useWindowResync() {
  const queryClient = useQueryClient();
  const syncRecording = useRecordingStore((s) => s.syncFromBackend);

  useEffect(() => {
    function handleResync() {
      // Invalidate queries so TanStack Query refreshes all active view queries
      void queryClient.invalidateQueries();
      // Resynchronize backend-owned recording lifecycle
      void syncRecording();
    }

    function handleVisibility() {
      if (document.visibilityState === "visible") {
        handleResync();
      }
    }

    window.addEventListener("focus", handleResync);
    document.addEventListener("visibilitychange", handleVisibility);

    // Initial sync
    void syncRecording();

    return () => {
      window.removeEventListener("focus", handleResync);
      document.removeEventListener("visibilitychange", handleVisibility);
    };
  }, [queryClient, syncRecording]);
}
