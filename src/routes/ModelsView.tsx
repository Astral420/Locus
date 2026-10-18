import React, { useCallback, useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "../components/layout/Header";
import { Skeleton } from "../components/ui/Skeleton";
import { SegmentedControl } from "../components/ui/SegmentedControl";
import { Toggle } from "../components/ui/Toggle";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { ModelCard, type DownloadState } from "../components/models/ModelCard";
import {
  cancelModelDownload,
  downloadModel,
  listModels,
  onModelDownloadProgress,
  refreshModelCatalog,
  selectModel,
  type ModelAssetDTO,
} from "../lib/tauri";
import { useUiStore } from "../stores/uiStore";
import { Info, X } from "lucide-react";

type ModelTab = "whisper" | "llm" | "embedding";

/** One-line reminder of what each category's model actually does. */
const TAB_PURPOSE: Record<ModelTab, string> = {
  whisper: "Turns recordings into transcripts.",
  llm: "Writes meeting summaries and answers questions in chat.",
  embedding: "Powers knowledge search.",
};

/** On this machine already (downloaded/imported, or currently selected). */
export const isInstalled = (model: ModelAssetDTO): boolean =>
  model.status === "installed" || model.status === "active";

export const ModelsContent: React.FC = () => {
  const [activeTab, setActiveTab] = useState<ModelTab>("whisper");
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const queryClient = useQueryClient();

  const { data: models, isLoading } = useQuery<ModelAssetDTO[]>({
    queryKey: ["models"],
    queryFn: listModels,
  });
  // Pull a newer catalog in the background; the list above keeps working from the cached copy.
  useEffect(() => {
    let cancelled = false;
    refreshModelCatalog()
      .then((report) => {
        if (!cancelled && report.updated.length > 0) {
          void queryClient.invalidateQueries({ queryKey: ["models"] });
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [queryClient]);

  // Downloads in flight, keyed by catalog id. Events also cover downloads the app starts itself
  // (first-run provisioning), so a card shows progress whichever way the download began.
  const [downloads, setDownloads] = useState<Record<string, DownloadState>>({});
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onModelDownloadProgress((event) => {
      setDownloads((current) => {
        const next = { ...current };
        if (event.state === "done") {
          delete next[event.catalog_id];
        } else {
          next[event.catalog_id] = {
            variantId: event.variant_id,
            state: event.state,
            received: event.received,
            total: event.total,
            error: event.error,
          };
        }
        return next;
      });
      if (event.state === "done") void queryClient.invalidateQueries({ queryKey: ["models"] });
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
  const startDownload = useCallback(
    (catalogId: string, variantId?: string) => {
      setDownloads((c) => ({ ...c, [catalogId]: { variantId: variantId ?? "", state: "running", received: 0, total: 0 } }));
      downloadModel(catalogId, variantId).catch((error: unknown) => {
        // The backend also emits a "failed" event; this covers a request that never started.
        setDownloads((c) =>
          c[catalogId]?.state === "failed"
            ? c
            : { ...c, [catalogId]: { variantId: variantId ?? "", state: "failed", received: 0, total: 0, error: String(error) } },
        );
      });
    },
    [],
  );
  const cancelDownload = useCallback((catalogId: string) => void cancelModelDownload(catalogId), []);

  // A removal that takes the last installed variant of a role changes what the app can still do,
  // so the card's dialog spells that out. Counts are across every model, not just this card's.
  const handleModelChanged = useCallback(
    (message: string) => {
      setStatusMessage(message);
      void queryClient.invalidateQueries({ queryKey: ["models"] });
    },
    [queryClient],
  );

  const noticeDismissed = useUiStore((s) => s.coreEngineNoticeDismissed);
  const dismissNotice = useUiStore((s) => s.dismissCoreEngineNotice);
  const installedOnly = useUiStore((s) => s.modelsInstalledOnly);
  const setInstalledOnly = useUiStore((s) => s.setModelsInstalledOnly);

  if (isLoading) {
    return (
      <div className="p-6 max-w-[742px] mx-auto w-full space-y-2">
        <Skeleton className="h-10 w-64" />
        <Skeleton className="h-36 w-full" />
        <Skeleton className="h-36 w-full" />
      </div>
    );
  }

  const inTab = (models || []).filter((m) => m.role === activeTab);
  const installedCount = inTab.filter(isInstalled).length;
  const filteredModels = installedOnly ? inTab.filter(isInstalled) : inTab;

  const installedVariants = (role: ModelTab) =>
    (models || [])
      .filter((m) => m.role === role)
      .flatMap((m) => m.variants ?? [])
      .filter((v) => v.installed === true).length;
  const whisperVariantsInstalled = installedVariants("whisper");
  const llmVariantsInstalled = installedVariants("llm");

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <Header
        title="Model Manager"
        subtitle="Offline models that power transcription, meeting summaries and chat, and knowledge search"
      />

      <div className="px-6 pt-2 pb-8 max-w-[742px] mx-auto w-full space-y-2 flex-1 overflow-y-auto">
        {!noticeDismissed && (
          <div
            role="region"
            aria-label="Offline-Ready Core Engine"
            className="flex items-start gap-3 rounded-xl bg-surface px-4 py-3 text-[13px] text-ink-muted leading-relaxed"
          >
            <Info className="w-4 h-4 text-primary shrink-0 mt-0.5" />
            <div className="flex-1">
              <strong className="text-ink font-medium block mb-0.5">Offline-Ready Core Engine</strong>
              Locus downloads Whisper Small and a small embedding model the first time it runs; after that everything
              works offline. Speaker diarization ships with the app. Other models can be downloaded at any time without
              blocking recording.
            </div>
            <button
              type="button"
              onClick={dismissNotice}
              aria-label="Dismiss notice"
              className="shrink-0 -mr-1 -mt-0.5 p-1.5 rounded-full text-ink-muted hover:text-ink hover:bg-surface-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        )}

        <div className="py-2 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
          <SegmentedControl<ModelTab>
            aria-label="Model category"
            value={activeTab}
            onChange={setActiveTab}
            options={[
              { value: "whisper", label: "Whisper Transcription" },
              { value: "llm", label: "Local Language Model" },
              { value: "embedding", label: "Embedding (RAG)" },
            ]}
          />
          <label className="inline-flex items-center gap-2 text-[13px] text-ink-muted cursor-pointer select-none">
            <span>
              Installed only
              <span className="ml-1.5 nums-tabular text-ink-subtle" aria-hidden="true">
                {installedCount}
              </span>
            </span>
            <Toggle checked={installedOnly} onChange={setInstalledOnly} label="Show installed models only" />
          </label>
        </div>

        <p data-testid="tab-purpose" className="px-0.5 text-[13px] text-ink-muted">
          {TAB_PURPOSE[activeTab]}
        </p>

        {statusMessage && (
          <p role="status" className="rounded-xl bg-surface px-3 py-2 text-[13px] text-ink-muted">
            {statusMessage}
          </p>
        )}

        {installedOnly && filteredModels.length === 0 && (
          <div role="status" className="rounded-xl bg-surface px-4 py-6 text-center text-[13px] text-ink-muted">
            No {activeTab === "whisper" ? "Whisper" : activeTab === "llm" ? "language" : "embedding"} models are installed yet.
            Turn off “Installed only” to browse what is available.
          </div>
        )}

        {filteredModels.map((model) => (
          <ModelCard
            key={model.id}
            model={model}
            download={model.catalog_id ? downloads[model.catalog_id] : undefined}
            onDownload={startDownload}
            onCancel={cancelDownload}
            onChanged={handleModelChanged}
            whisperVariantsInstalled={whisperVariantsInstalled}
            llmVariantsInstalled={llmVariantsInstalled}
            onSelect={(id) =>
              void selectModel(id, activeTab).then(() => queryClient.invalidateQueries({ queryKey: ["models"] }))
            }
          />
        ))}
      </div>
    </div>
  );
};

export const ModelsView: React.FC = () => {
  return (
    <ErrorBoundary fallbackTitle="Unable to load Model Manager">
      <ModelsContent />
    </ErrorBoundary>
  );
};
