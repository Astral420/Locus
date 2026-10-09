import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { Skeleton } from "../components/ui/Skeleton";
import { ProgressBar } from "../components/ui/ProgressBar";
import { SegmentedControl } from "../components/ui/SegmentedControl";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { listModels, selectModel, type ModelAssetDTO } from "../lib/tauri";
import { useUiStore } from "../stores/uiStore";
import { Download, CheckCircle, ChevronDown, Info, Layers, X } from "lucide-react";

type ModelTab = "whisper" | "llm" | "embedding";

const STATUS_LABEL: Record<ModelAssetDTO["status"], string> = {
  bundled: "Bundled",
  installed: "Installed",
  active: "Active",
  downloading: "Downloading",
  available: "Available",
  error: "Error",
};

/** One Hub-style model card. Catalog fields (author, downloads, variants, tags, fits) render only when present. */
const ModelCard: React.FC<{ model: ModelAssetDTO; onSelect: (id: string) => void }> = ({ model, onSelect }) => {
  const [showVariants, setShowVariants] = useState(false);
  const variants = model.variants ?? [];
  const canSelect = model.status !== "available" && model.status !== "bundled";

  return (
    <article className="rounded-xl bg-surface p-4 space-y-3" aria-label={model.name}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[15px] font-medium text-ink flex items-center gap-2 flex-wrap">
            {model.name}
            {model.quantization && (
              <Badge size="sm" className="font-mono">
                {model.quantization}
              </Badge>
            )}
          </h2>
        </div>

        <div className="flex flex-col items-end gap-2 shrink-0">
          <div className="flex items-center gap-2.5">
            <span className="text-xs text-ink-muted nums-tabular">{model.disk_size}</span>
            {model.fits !== undefined && (
              <Badge variant={model.fits ? "green" : "amber"} size="sm">
                {model.fits ? "✓ Fits" : "May not fit"}
              </Badge>
            )}
            {model.status !== "active" && (
              <Badge size="sm">{STATUS_LABEL[model.status]}</Badge>
            )}
          </div>

          {model.status === "active" ? (
            <span className="inline-flex items-center gap-1.5 h-8 px-3.5 rounded-full bg-green-tint text-green-text text-xs font-medium">
              <CheckCircle className="w-3.5 h-3.5" /> Active
            </span>
          ) : model.status === "downloading" ? (
            <div className="w-40 space-y-1">
              <ProgressBar value={model.download_progress ?? 0} label={`Downloading ${model.name}`} />
              <div className="text-[11px] text-ink-muted text-right nums-tabular">{Math.round(model.download_progress ?? 0)}%</div>
            </div>
          ) : (
            <Button
              variant={canSelect ? "primary" : "secondary"}
              size="sm"
              disabled={!canSelect}
              onClick={() => canSelect && onSelect(model.id)}
            >
              <Download className="w-3.5 h-3.5" />
              {model.status === "available" ? "Provision in Settings" : "Set Active"}
            </Button>
          )}
        </div>
      </div>

      <p className="text-[13px] text-ink-muted leading-relaxed max-w-[560px]">
        {model.description ?? `Parameters: ${model.parameter_size}`}
      </p>

      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="flex items-center gap-3 flex-wrap text-xs text-ink-muted">
          {model.author && <span>By {model.author}</span>}
          {model.downloads !== undefined && (
            <span className="inline-flex items-center gap-1 nums-tabular" title="Downloads">
              <Download className="w-3.5 h-3.5" aria-hidden="true" />
              {model.downloads.toLocaleString("en-US")}
            </span>
          )}
          {variants.length > 0 && (
            <span className="inline-flex items-center gap-1 nums-tabular" title="Variants">
              <Layers className="w-3.5 h-3.5" aria-hidden="true" />
              {variants.length}
            </span>
          )}
          {(model.tags ?? []).map((tag) => (
            <Badge key={tag} size="sm">
              {tag}
            </Badge>
          ))}
          <span className="font-mono text-[11px] text-ink-subtle">
            RAM {model.memory_ram} · VRAM {model.memory_vram}
          </span>
        </div>

        {variants.length > 0 && (
          <button
            type="button"
            aria-expanded={showVariants}
            onClick={() => setShowVariants((v) => !v)}
            className="inline-flex items-center gap-1 text-xs text-ink hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded"
          >
            Show variants
            <ChevronDown className={`w-3.5 h-3.5 transition-transform ${showVariants ? "rotate-180" : ""}`} />
          </button>
        )}
      </div>

      {showVariants && (
        <ul className="divide-y divide-border-subtle rounded-xl bg-bg px-3" aria-label={`${model.name} variants`}>
          {variants.map((v) => (
            <li key={v.quantization} className="flex items-center justify-between gap-3 py-2.5 text-xs">
              <span className="font-mono text-ink">{v.quantization}</span>
              <span className="text-ink-muted nums-tabular">
                {v.disk_size}
                {v.memory_ram ? ` · RAM ${v.memory_ram}` : ""}
              </span>
              <Button
                variant="secondary"
                size="sm"
                disabled
                title="In-app downloads are not available yet"
                aria-label={`Download ${v.quantization}`}
              >
                Download · {v.quantization}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </article>
  );
};

export const ModelsContent: React.FC = () => {
  const [activeTab, setActiveTab] = useState<ModelTab>("whisper");
  const queryClient = useQueryClient();

  const { data: models, isLoading } = useQuery<ModelAssetDTO[]>({
    queryKey: ["models"],
    queryFn: listModels,
  });
  const noticeDismissed = useUiStore((s) => s.coreEngineNoticeDismissed);
  const dismissNotice = useUiStore((s) => s.dismissCoreEngineNotice);

  if (isLoading) {
    return (
      <div className="p-6 max-w-[742px] mx-auto w-full space-y-2">
        <Skeleton className="h-10 w-64" />
        <Skeleton className="h-36 w-full" />
        <Skeleton className="h-36 w-full" />
      </div>
    );
  }

  const filteredModels = (models || []).filter((m) => m.role === activeTab);

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <Header
        title="Model Manager"
        subtitle="Offline weights for Whisper transcription, LLM summarization, and RAG embeddings"
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
              Locus includes bundled Whisper small transcription and pyannote diarization that work immediately out of
              the box with zero internet connection. Additional generation models can be downloaded at your convenience
              without blocking recording.
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

        <div className="py-2">
          <SegmentedControl<ModelTab>
            aria-label="Model category"
            value={activeTab}
            onChange={setActiveTab}
            options={[
              { value: "whisper", label: "Whisper Transcription" },
              { value: "llm", label: "Summarization (llama-server)" },
              { value: "embedding", label: "Embedding (RAG)" },
            ]}
          />
        </div>

        {filteredModels.map((model) => (
          <ModelCard
            key={model.id}
            model={model}
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
