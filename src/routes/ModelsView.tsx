import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { Skeleton } from "../components/ui/Skeleton";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { getGpuBackend, listModels, selectModel, type ModelAssetDTO } from "../lib/tauri";
import {
  Cpu,
  Download,
  CheckCircle,
  HardDrive,
  Layers,
  Sparkles,
  Info,
  Upload,
} from "lucide-react";

export const ModelsContent: React.FC = () => {
  const [activeTab, setActiveTab] = useState<"whisper" | "llm" | "embedding">("whisper");
  const queryClient = useQueryClient();

  const { data: models, isLoading } = useQuery<ModelAssetDTO[]>({
    queryKey: ["models"],
    queryFn: listModels,
  });
  const { data: gpu } = useQuery({ queryKey: ["gpu-backend"], queryFn: getGpuBackend });

  if (isLoading) {
    return (
      <div className="p-6 max-w-5xl mx-auto w-full space-y-4">
        <Skeleton className="h-10 w-64" />
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <Skeleton className="h-44 w-full" />
          <Skeleton className="h-44 w-full" />
        </div>
      </div>
    );
  }

  const filteredModels = (models || []).filter((m) => m.role === activeTab);

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-bg">
      <Header
        title="Unified Model Manager"
        subtitle="Manage offline local weights for Whisper transcription, LLM summarization, and RAG embeddings"
        actions={
          <Button variant="secondary" size="sm" onClick={() => alert("Select .gguf file dialog")}>
            <Upload className="w-3.5 h-3.5 mr-1" />
            Import Local GGUF
          </Button>
        }
      />

      <div className="p-6 max-w-5xl mx-auto w-full space-y-6 flex-1 overflow-y-auto">
        {/* Skippable Offline Notice Banner */}
        <div className="p-4 rounded-lg bg-green-tint/40 border border-green-text/20 flex items-start gap-3 text-xs text-ink leading-relaxed">
          <Info className="w-4 h-4 text-primary shrink-0 mt-0.5" />
          <div>
            <strong className="text-green-text block mb-0.5">Offline-Ready Core Engine</strong>
            Locus includes bundled Whisper small transcription and pyannote diarization that work immediately out of the box with zero internet connection. Additional generation models can be downloaded at your convenience without blocking recording.
          </div>
        </div>

        {gpu && (
          <div className="flex items-center justify-between rounded-lg border border-border bg-surface px-4 py-3 text-xs">
            <div>
              <span className="font-semibold text-ink">Inference backend: {gpu.backend.toUpperCase()}</span>
              <span className="ml-2 text-ink-muted">{gpu.reason}</span>
            </div>
            <Badge variant={gpu.backend === "cpu" ? "neutral" : "green"} size="sm">
              {gpu.architecture}
            </Badge>
          </div>
        )}

        {/* Categorized Tab Navigation */}
        <div className="flex items-center gap-1 border-b border-border">
          <button
            type="button"
            onClick={() => setActiveTab("whisper")}
            className={`h-9 px-4 text-xs font-semibold border-b-2 transition-colors flex items-center gap-2 ${
              activeTab === "whisper"
                ? "border-primary text-primary"
                : "border-transparent text-ink-muted hover:text-ink"
            }`}
          >
            <Layers className="w-4 h-4" />
            Whisper Transcription Models
          </button>
          <button
            type="button"
            onClick={() => setActiveTab("llm")}
            className={`h-9 px-4 text-xs font-semibold border-b-2 transition-colors flex items-center gap-2 ${
              activeTab === "llm"
                ? "border-primary text-primary"
                : "border-transparent text-ink-muted hover:text-ink"
            }`}
          >
            <Sparkles className="w-4 h-4" />
            Summarization Engines (llama-server)
          </button>
          <button
            type="button"
            onClick={() => setActiveTab("embedding")}
            className={`h-9 px-4 text-xs font-semibold border-b-2 transition-colors flex items-center gap-2 ${
              activeTab === "embedding"
                ? "border-primary text-primary"
                : "border-transparent text-ink-muted hover:text-ink"
            }`}
          >
            <Cpu className="w-4 h-4" />
            Embedding Models (ChromaDB RAG)
          </button>
        </div>

        {/* Model Cards Grid */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {filteredModels.map((model) => (
            <div
              key={model.id}
              className="p-5 rounded-lg border border-border bg-surface-elevated flex flex-col justify-between gap-4 hover:border-border-active/60 transition-colors shadow-xs"
            >
              <div>
                <div className="flex items-start justify-between gap-3 mb-2">
                  <div>
                    <h2 className="text-sm font-semibold text-ink flex items-center gap-2">
                      {model.name}
                      {model.quantization && (
                        <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-surface-sunken text-ink-muted border border-border">
                          {model.quantization}
                        </span>
                      )}
                    </h2>
                    <span className="text-xs text-ink-muted font-mono mt-0.5 block">
                      Parameters: {model.parameter_size}
                    </span>
                  </div>
                  <Badge
                    variant={model.status === "active" ? "green" : "neutral"}
                    size="sm"
                  >
                    {model.status.toUpperCase()}
                  </Badge>
                </div>

                {/* Resource Footprint Specs */}
                <div className="grid grid-cols-3 gap-2 p-2.5 rounded bg-surface-sunken border border-border/70 text-xs font-mono text-ink-muted mt-3">
                  <div>
                    <span className="text-[10px] uppercase text-ink-subtle block">RAM</span>
                    <span className="font-semibold text-ink">{model.memory_ram}</span>
                  </div>
                  <div>
                    <span className="text-[10px] uppercase text-ink-subtle block">VRAM</span>
                    <span className="font-semibold text-ink">{model.memory_vram}</span>
                  </div>
                  <div>
                    <span className="text-[10px] uppercase text-ink-subtle block">Disk</span>
                    <span className="font-semibold text-ink">{model.disk_size}</span>
                  </div>
                </div>
              </div>

              {/* Action Buttons */}
              <div className="flex items-center justify-between pt-2 border-t border-border/60">
                <span className="text-[11px] text-ink-muted">
                  {model.status === "active"
                    ? "Currently serving requests"
                    : model.status === "bundled"
                    ? "Read-only bundled asset"
                    : "Available for offline install"}
                </span>

                {model.status === "active" ? (
                  <span className="text-xs font-medium text-green-text flex items-center gap-1">
                    <CheckCircle className="w-3.5 h-3.5 text-primary" /> Active
                  </span>
                ) : (
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={model.status === "available" || model.status === "bundled"}
                    onClick={() => {
                      if (model.status !== "available" && model.status !== "bundled") {
                        void selectModel(model.id, activeTab).then(() => queryClient.invalidateQueries({ queryKey: ["models"] }));
                      }
                    }}
                  >
                    <Download className="w-3.5 h-3.5 mr-1" />
                    {model.status === "available" ? "Provision in Settings" : "Set Active"}
                  </Button>
                )}
              </div>
            </div>
          ))}
        </div>
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
