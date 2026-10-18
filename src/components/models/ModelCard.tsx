import React, { useState } from "react";
import { Button } from "../ui/Button";
import { Badge } from "../ui/Badge";
import { ProgressBar } from "../ui/ProgressBar";
import { Download, CheckCircle, ChevronDown, Layers, X } from "lucide-react";
import type { ModelAssetDTO } from "../../lib/tauri";

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
}> = ({ model, onSelect, download, onDownload, onCancel }) => {
  const [showVariants, setShowVariants] = useState(false);
  const variants = model.variants ?? [];
  const catalogId = model.catalog_id;
  const downloading = download?.state === "running";
  const canDownload = model.status === "available" && !!catalogId && variants.length > 0 && !downloading;
  const installedHere = model.status === "installed" || model.status === "active";
  // The variant behind the button on the card is the one the card summarises (e.g. Q4_K_M).
  const headline =
    variants.find((v) => v.quantization === model.quantization) ??
    variants.find((v) => v.active) ??
    variants[0];

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
              </li>
            );
          })}
        </ul>
      )}
    </article>
  );
};
