import React, { useState } from "react";
import { Button } from "../ui/Button";
import { Badge } from "../ui/Badge";
import { ProgressBar } from "../ui/ProgressBar";
import { Modal } from "../ui/Modal";
import { formatBytes } from "../../lib/format";
import { Download, CheckCircle, ChevronDown, Layers, X, Trash2 } from "lucide-react";
import { uninstallModel, type ModelAssetDTO, type ModelVariantDTO } from "../../lib/tauri";

const STATUS_LABEL: Record<ModelAssetDTO["status"], string> = {
  installed: "Installed",
  active: "Active",
  downloading: "Downloading",
  available: "Available",
  error: "Error",
};

/** Live state of one download, keyed by catalog id. */
export interface DownloadState {
  variantId: string;
  state: "running" | "failed" | "canceled";
  received: number;
  total: number;
  error?: string;
}

const percent = (d: DownloadState): number => (d.total > 0 ? Math.min(100, (d.received / d.total) * 100) : 0);

/** One Hub-style model card. Catalog fields (author, downloads, variants, tags, fits) render only when present. */
export const ModelCard: React.FC<{
  model: ModelAssetDTO;
  onSelect: (id: string) => void;
  download?: DownloadState;
  onDownload: (catalogId: string, variantId?: string) => void;
  onCancel: (catalogId: string) => void;
  /** Called after a successful removal, so the view can report it and refetch the list. */
  onChanged?: (message: string) => void;
  /** Installed variant counts across every model of the role, so the dialog can warn before the last one goes. */
  whisperVariantsInstalled?: number;
  llmVariantsInstalled?: number;
}> = ({ model, onSelect, download, onDownload, onCancel, onChanged, whisperVariantsInstalled, llmVariantsInstalled }) => {
  const [showVariants, setShowVariants] = useState(false);
  const [pending, setPending] = useState<{ variants: ModelVariantDTO[]; all: boolean } | null>(null);
  const [isRemoving, setIsRemoving] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const variants = model.variants ?? [];
  const catalogId = model.catalog_id;
  const downloading = download?.state === "running";
  const canDownload = model.status === "available" && !!catalogId && variants.length > 0 && !downloading;
  const installedHere = model.status === "installed" || model.status === "active";
  // Only files Locus owns may be removed; the backend never deletes outside its managed folders.
  const removableVariants = variants.filter((v) => v.installed === true && v.removable === true);
  // The variant behind the button on the card is the one the card summarises (e.g. Q4_K_M).
  const headline =
    variants.find((v) => v.quantization === model.quantization) ??
    variants.find((v) => v.active) ??
    variants[0];

  const closeDialog = () => {
    if (isRemoving) return;
    setRemoveError(null);
    setPending(null);
  };

  const removalLines = (targets: ModelVariantDTO[]): string[] => {
    const lines = [
      `This deletes ${targets.length} ${targets.length === 1 ? "file" : "files"} from this computer. You can download it again at any time.`,
    ];
    if (targets.some((v) => v.active === true)) {
      lines.push("Locus will switch to another installed quantization of this model if there is one.");
    }
    if (model.role === "whisper" && whisperVariantsInstalled !== undefined && targets.length >= whisperVariantsInstalled) {
      lines.push("Transcription is unavailable until a Whisper model is installed. Recordings are still saved.");
    }
    if (model.role === "llm" && llmVariantsInstalled !== undefined && targets.length >= llmVariantsInstalled) {
      lines.push("Summaries and chat are unavailable until a language model is installed or another provider is selected.");
    }
    if (model.role === "embedding") {
      lines.push(
        "Knowledge search stops working until an embedding model is selected. Your existing index is kept; installing the same model again restores it.",
      );
    }
    return lines;
  };

  const handleConfirm = async () => {
    if (!pending || !catalogId) return;
    setIsRemoving(true);
    setRemoveError(null);
    try {
      const result = await uninstallModel(catalogId, pending.all ? undefined : pending.variants[0].id);
      const quants = result.removed
        .map((id) => variants.find((v) => v.id === id)?.quantization ?? id)
        .join(", ");
      onChanged?.(`Removed ${model.name} (${quants}). Freed ${formatBytes(result.freed_bytes)}.`);
      setPending(null);
    } catch (reason) {
      setRemoveError(
        reason instanceof Error ? reason.message : typeof reason === "string" ? reason : "The model could not be removed.",
      );
    } finally {
      setIsRemoving(false);
    }
  };

  return (
    <article className="rounded-xl bg-surface p-4 space-y-3" aria-label={model.name}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[15px] font-medium text-ink flex items-center gap-2 flex-wrap">
            {model.name}
            {model.is_default && (
              <Badge size="sm">
                Default
              </Badge>
            )}
            {model.quantization && (
              <Badge size="sm" className="font-mono">
                {installedHere ? `Using ${model.quantization}` : model.quantization}
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
          ) : downloading && download ? (
            <div className="w-44 space-y-1">
              <ProgressBar value={percent(download)} label={`Downloading ${model.name}`} />
              <div className="flex items-center justify-between gap-2 text-[11px] text-ink-muted nums-tabular">
                <button
                  type="button"
                  onClick={() => catalogId && onCancel(catalogId)}
                  className="inline-flex items-center gap-1 hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent rounded"
                  aria-label={`Cancel download of ${model.name}`}
                >
                  <X className="w-3 h-3" aria-hidden="true" /> Cancel
                </button>
                <span>{Math.round(percent(download))}%</span>
              </div>
            </div>
          ) : model.status === "downloading" ? (
            <div className="w-40 space-y-1">
              <ProgressBar value={model.download_progress ?? 0} label={`Downloading ${model.name}`} />
              <div className="text-[11px] text-ink-muted text-right nums-tabular">{Math.round(model.download_progress ?? 0)}%</div>
            </div>
          ) : model.status === "installed" ? (
            <Button
              variant="primary"
              size="sm"
              onClick={() => onSelect(headline?.select_id ?? model.id)}
            >
              <CheckCircle className="w-3.5 h-3.5" />
              Set active
            </Button>
          ) : model.status === "available" ? (
            <Button
              variant={canDownload ? "primary" : "secondary"}
              size="sm"
              disabled={!canDownload}
              title={!canDownload ? "Not downloadable until the model catalog has loaded" : undefined}
              onClick={() => {
                if (canDownload && catalogId) onDownload(catalogId, headline?.id);
              }}
            >
              <Download className="w-3.5 h-3.5" />
              {download?.state === "failed" ? "Retry download" : "Download"}
            </Button>
          ) : null}
          {download?.state === "failed" && (
            <p role="alert" className="max-w-[260px] text-right text-[11px] text-red-text">
              Download failed: {download.error}
            </p>
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

      {removableVariants.length > 0 && (
        <div className="flex items-center justify-end">
          <Button
            variant="secondary"
            size="sm"
            disabled={downloading}
            onClick={() => setPending({ variants: removableVariants, all: true })}
          >
            <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
            {removableVariants.length === 1 ? "Uninstall" : `Uninstall all (${removableVariants.length})`}
          </Button>
        </div>
      )}

      {showVariants && (
        <ul className="divide-y divide-border-subtle rounded-xl bg-bg px-3" aria-label={`${model.name} variants`}>
          {variants.map((v) => {
            const isActive = v.active === true;
            const isInstalled = v.installed === true;
            const downloadable = !isInstalled && !!catalogId && !!v.id && !downloading;
            return (
              <li key={v.id ?? v.quantization} className="flex items-center justify-between gap-3 py-2.5 text-xs">
                <span className="inline-flex items-center gap-2 min-w-0">
                  <span className="font-mono text-ink">{v.quantization}</span>
                  {isInstalled && <Badge size="sm">Installed</Badge>}
                </span>
                <span className="text-ink-muted nums-tabular ml-auto text-right">
                  {v.disk_size}
                  {v.memory_ram ? ` · RAM ${v.memory_ram}` : ""}
                </span>
                <span className="flex items-center gap-2">
                  {isActive ? (
                    <span
                      className="inline-flex items-center gap-1 h-7 px-3 rounded-full bg-green-tint text-green-text text-[11px] font-medium"
                      aria-label={`${v.quantization} active`}
                    >
                      <CheckCircle className="w-3 h-3" aria-hidden="true" /> Active
                    </span>
                  ) : isInstalled ? (
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={!v.select_id}
                      aria-label={`Set ${v.quantization} active`}
                      onClick={() => v.select_id && onSelect(v.select_id)}
                    >
                      Set active
                    </Button>
                  ) : (
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={!downloadable}
                      title={!catalogId ? "Not downloadable until the model catalog has loaded" : undefined}
                      aria-label={`Download ${v.quantization}`}
                      onClick={() => {
                        if (catalogId && v.id) onDownload(catalogId, v.id);
                      }}
                    >
                      Download · {v.quantization}
                    </Button>
                  )}
                  {isInstalled && v.removable === true ? (
                    <button
                      type="button"
                      disabled={downloading}
                      aria-label={`Remove ${v.quantization}`}
                      onClick={() => setPending({ variants: [v], all: false })}
                      className="inline-flex items-center justify-center w-7 h-7 rounded-full text-ink-muted hover:text-status-error hover:bg-surface-hover disabled:opacity-50 disabled:pointer-events-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                      <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
                    </button>
                  ) : isInstalled ? (
                    <span
                      className="text-[11px] text-ink-subtle"
                      title="This file is not in Locus's model folder, so Locus won't delete it."
                    >
                      Managed outside Locus
                    </span>
                  ) : null}
                </span>
              </li>
            );
          })}
        </ul>
      )}

      <Modal
        isOpen={pending !== null}
        onClose={closeDialog}
        title={pending?.all ? `Uninstall ${model.name}?` : pending ? `Remove ${model.name} · ${pending.variants[0].quantization}?` : ""}
      >
        <div className="space-y-3 text-sm text-ink-muted">
          {removeError && (
            <p role="alert" className="rounded-xl bg-status-error/10 px-3 py-2 text-xs text-status-error">
              {removeError}
            </p>
          )}
          {pending?.variants &&
            removalLines(pending.variants).map((line) => <p key={line}>{line}</p>)}
        </div>
        <div className="mt-4 flex items-center justify-end gap-2">
          <Button variant="secondary" size="sm" onClick={closeDialog} disabled={isRemoving}>
            Cancel
          </Button>
          <Button variant="danger" size="sm" onClick={() => void handleConfirm()} isLoading={isRemoving}>
            <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
            {isRemoving ? "Removing…" : pending?.all ? "Uninstall" : "Remove"}
          </Button>
        </div>
      </Modal>
    </article>
  );
};
