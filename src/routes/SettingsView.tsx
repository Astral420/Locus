import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { Toggle } from "../components/ui/Toggle";
import { SelectPill } from "../components/ui/SelectPill";
import { PathChip } from "../components/ui/PathChip";
import { SettingsCard, SettingsRow, SectionLabel } from "../components/ui/SettingsCard";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { useUiStore, type ThemeMode } from "../stores/uiStore";
import { checkForUpdate, configureProvider, getGpuBackend, getStorageInfo, listProviderConfigs, migrateModels, saveProviderApiKey, type GpuBackendDTO, type ProviderConfigDTO, type StorageInfoDTO } from "../lib/tauri";
import { Settings, Palette, Mic, HardDrive, Shield, Lock, Cpu, Server, Cloud } from "lucide-react";

type RemoteProviderId = "ollama" | "openai" | "anthropic" | "gemini";
type ProviderId = "llama_server" | RemoteProviderId;
type SettingsCategory = "general" | "appearance" | "recording" | "storage" | "privacy" | ProviderId;

interface RemoteProviderSpec {
  id: RemoteProviderId;
  label: string;
  icon: typeof Cloud;
  blurb: string;
  endpoint: string;
  defaultModel: string;
  needsKey: boolean;
}

/** Remote providers supported by the backend (PRD/SPEC: Ollama, OpenAI, Anthropic, Gemini). Endpoints mirror the backend defaults. */
const REMOTE_PROVIDERS: RemoteProviderSpec[] = [
  { id: "ollama", label: "Ollama", icon: Server, blurb: "Connect to an external Ollama instance (no API key).", endpoint: "http://127.0.0.1:11434", defaultModel: "", needsKey: false },
  { id: "openai", label: "OpenAI", icon: Cloud, blurb: "Cloud API client, requires user API key", endpoint: "https://api.openai.com", defaultModel: "gpt-4o-mini", needsKey: true },
  { id: "anthropic", label: "Anthropic", icon: Cloud, blurb: "Cloud API client, requires user API key", endpoint: "https://api.anthropic.com", defaultModel: "", needsKey: true },
  { id: "gemini", label: "Gemini", icon: Cloud, blurb: "Cloud API client, requires user API key", endpoint: "https://generativelanguage.googleapis.com/v1beta", defaultModel: "", needsKey: true },
];

interface RemoteForm {
  endpoint?: string;
  model?: string;
  key?: string;
}

export const SettingsContent: React.FC = () => {
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("general");
  const queryClient = useQueryClient();
  const { theme, setTheme } = useUiStore();

  const [selectedProvider, setSelectedProvider] = useState<ProviderId>("llama_server");
  const [forms, setForms] = useState<Partial<Record<RemoteProviderId, RemoteForm>>>({});
  const [providerMessage, setProviderMessage] = useState<string | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [defaults, setDefaults] = useState({ systemAudio: true, screenVideo: true, threeHourBanner: true });
  const { data: providerConfigs = [] } = useQuery<ProviderConfigDTO[]>({ queryKey: ["provider-configs"], queryFn: listProviderConfigs });
  const { data: storage } = useQuery<StorageInfoDTO>({ queryKey: ["storage-info"], queryFn: getStorageInfo });
  const { data: gpu } = useQuery<GpuBackendDTO>({ queryKey: ["gpu-backend"], queryFn: getGpuBackend });

  const generalCategories = [
    { id: "general", label: "General", icon: Settings },
    { id: "appearance", label: "Appearance & Theme", icon: Palette },
    { id: "recording", label: "Recording Defaults", icon: Mic },
    { id: "storage", label: "Storage & Relocation", icon: HardDrive },
    { id: "privacy", label: "Privacy & Sovereignty", icon: Shield },
  ] as const;

  const inputClass =
    "w-full max-w-[640px] h-9 px-3 rounded-lg border border-border bg-bg text-xs font-mono text-ink placeholder:text-ink-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent";

  const configFor = (id: ProviderId) => providerConfigs.find((c) => c.provider === id);
  const providerLabel = (id: ProviderId) =>
    id === "llama_server" ? "Llama.cpp (local, offline)" : REMOTE_PROVIDERS.find((p) => p.id === id)!.label;

  const formValue = (spec: RemoteProviderSpec, field: "endpoint" | "model") => {
    const edited = forms[spec.id]?.[field];
    if (edited !== undefined) return edited;
    const saved = configFor(spec.id);
    if (saved) return field === "endpoint" ? saved.destination : saved.model;
    return field === "endpoint" ? spec.endpoint : spec.defaultModel;
  };
  const setForm = (id: RemoteProviderId, patch: RemoteForm) =>
    setForms((prev) => ({ ...prev, [id]: { ...prev[id], ...patch } }));

  const saveRemote = (spec: RemoteProviderSpec) => {
    const endpoint = formValue(spec, "endpoint").trim();
    const model = formValue(spec, "model").trim();
    const key = forms[spec.id]?.key ?? "";
    setProviderMessage(null);
    void configureProvider(spec.id, model, endpoint, true)
      .then(() => (spec.needsKey ? saveProviderApiKey(spec.id, key) : undefined))
      .then(() => {
        setProviderMessage(
          spec.needsKey
            ? `${spec.label} is configured in the operating-system keychain.`
            : `${spec.label} is configured.`
        );
        setForm(spec.id, { key: "" });
        void queryClient.invalidateQueries({ queryKey: ["provider-configs"] });
      })
      .catch((error: unknown) => setProviderMessage(error instanceof Error ? error.message : "Provider configuration failed."));
  };

  const navButtonClass = (isActive: boolean) =>
    `w-full h-9 px-3 rounded-[10px] text-sm flex items-center gap-2.5 transition-colors text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent ${
      isActive ? "bg-surface-hover text-ink" : "text-ink-muted hover:text-ink hover:bg-surface-hover/60"
    }`;

  const selectCategory = (id: SettingsCategory) => {
    setProviderMessage(null);
    setActiveCategory(id);
  };

  const providerNavButton = (id: ProviderId, label: string, Icon: typeof Cloud) => (
    <button
      key={id}
      type="button"
      onClick={() => selectCategory(id)}
      aria-current={activeCategory === id ? "page" : undefined}
      className={navButtonClass(activeCategory === id)}
    >
      <Icon className="w-4 h-4 shrink-0" />
      <span className="truncate flex-1">{label}</span>
      {configFor(id)?.configured && <span aria-hidden="true" className="w-1.5 h-1.5 rounded-full bg-primary shrink-0" />}
    </button>
  );

  const subLabel = (text: string) => (
    <div className="px-3 pt-2 pb-1 text-[11px] uppercase tracking-[0.08em] text-ink-subtle">{text}</div>
  );

  const policyCard = (
    <SettingsCard title="Authorization">
      <SettingsRow
        label={
          <span className="inline-flex items-center gap-2">
            <Lock className="w-4 h-4 text-primary" /> Sovereign Provider Policy (Zero Silent Remote Failover)
          </span>
        }
        description={
          <>
            Active selected provider: <strong className="text-ink font-medium">{providerLabel(selectedProvider)}</strong>.
            Configuring credentials makes remote APIs available, but Locus never sends meeting content until you select that provider. Credentials are saved exclusively in your OS keychain.
          </>
        }
      />
    </SettingsCard>
  );

  const selectButton = (id: ProviderId) => (
    <Button variant={selectedProvider === id ? "primary" : "secondary"} size="sm" onClick={() => setSelectedProvider(id)}>
      {selectedProvider === id ? "Selected (Active)" : "Select"}
    </Button>
  );

  const remoteSpec = REMOTE_PROVIDERS.find((p) => p.id === activeCategory);

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <Header
        title="Settings"
        subtitle="Appearance, capture defaults, providers, and storage"
      />

      <div className="flex-1 flex gap-4 px-6 pb-6 min-h-0 overflow-hidden">
        {/* In-page secondary navigation */}
        <nav aria-label="Settings categories" className="w-[220px] shrink-0 overflow-y-auto space-y-0.5">
          {generalCategories.map((cat) => {
            const Icon = cat.icon;
            return (
              <button
                key={cat.id}
                type="button"
                onClick={() => selectCategory(cat.id)}
                aria-current={activeCategory === cat.id ? "page" : undefined}
                className={navButtonClass(activeCategory === cat.id)}
              >
                <Icon className="w-4 h-4 shrink-0" />
                <span className="truncate">{cat.label}</span>
              </button>
            );
          })}

          <SectionLabel>Providers</SectionLabel>
          {subLabel("Local")}
          {providerNavButton("llama_server", "Llama.cpp", Cpu)}
          {subLabel("Remote")}
          {REMOTE_PROVIDERS.map((p) => providerNavButton(p.id, p.label, p.icon))}
        </nav>

        {/* Grouped cards */}
        <div className="flex-1 min-w-0 overflow-y-auto space-y-3">
          {/* GENERAL */}
          {activeCategory === "general" && (
            <SettingsCard title="General">
              <SettingsRow
                label="Configured providers"
                description="Locus keeps capture local by default and never enables a remote destination implicitly."
                control={<span className="text-sm font-medium text-ink nums-tabular">{providerConfigs.filter((config) => config.configured).length}</span>}
              />
              <SettingsRow
                label="Updates"
                description="Check the stable channel for a newer version of Locus."
                control={
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() =>
                      void checkForUpdate()
                        .then((result) =>
                          setUpdateMessage(
                            result.available_version
                              ? `Version ${result.available_version} is available on the stable channel.`
                              : "Locus is up to date on the stable channel."
                          )
                        )
                        .catch((error: unknown) => setUpdateMessage(error instanceof Error ? error.message : "Update check failed."))
                    }
                  >
                    Check stable channel
                  </Button>
                }
              >
                {updateMessage && <p role="status" className="text-xs text-green-text">{updateMessage}</p>}
              </SettingsRow>
            </SettingsCard>
          )}

          {/* APPEARANCE */}
          {activeCategory === "appearance" && (
            <SettingsCard title="Appearance">
              <SettingsRow
                label="Theme"
                description="System follows your OS. Light is the paper studio; dark is the graphite studio."
                control={
                  <SelectPill
                    aria-label="Theme"
                    value={theme}
                    onChange={(v) => setTheme(v as ThemeMode)}
                    options={[
                      { value: "system", label: "System" },
                      { value: "light", label: "Light" },
                      { value: "dark", label: "Dark" },
                    ]}
                  />
                }
              />
            </SettingsCard>
          )}

          {/* RECORDING DEFAULTS */}
          {activeCategory === "recording" && (
            <SettingsCard title="Capture Defaults">
              <SettingsRow
                label="Default System Audio"
                description="Always pre-select System Audio on session launch"
                control={<Toggle label="Default System Audio" checked={defaults.systemAudio} onChange={(v) => setDefaults((d) => ({ ...d, systemAudio: v }))} />}
              />
              <SettingsRow
                label="Default Screen Video"
                description="Enable automatic slide extraction via OpenCV differencing"
                control={<Toggle label="Default Screen Video" checked={defaults.screenVideo} onChange={(v) => setDefaults((d) => ({ ...d, screenVideo: v }))} />}
              />
              <SettingsRow
                label="3-Hour Advisory Banner"
                description="Show non-blocking advisory notification without interrupting recording"
                control={<Toggle label="3-Hour Advisory Banner" checked={defaults.threeHourBanner} onChange={(v) => setDefaults((d) => ({ ...d, threeHourBanner: v }))} />}
              />
            </SettingsCard>
          )}

          {/* PROVIDERS: LOCAL / Llama.cpp */}
          {activeCategory === "llama_server" && (
            <>
              {policyCard}
              <SettingsCard title="Llama.cpp">
                <SettingsRow
                  label="Local llama-server"
                  description="Private loopback inference, zero data leaves machine"
                  control={selectButton("llama_server")}
                />
              </SettingsCard>
              <SettingsCard title="Inference Backend">
                <SettingsRow
                  label={`Backend: ${gpu ? gpu.backend.toUpperCase() : "Detecting…"}`}
                  description={gpu?.reason ?? "Probing the hardware that runs local summarization and embeddings."}
                  control={gpu ? <Badge variant={gpu.backend === "cpu" ? "neutral" : "green"} size="sm">{gpu.backend.toUpperCase()}</Badge> : undefined}
                />
                {gpu && (
                  <SettingsRow label="Architecture" control={<Badge size="sm">{gpu.architecture}</Badge>} />
                )}
                {gpu?.device && <SettingsRow label="Device" control={<span className="text-xs text-ink-muted">{gpu.device}</span>} />}
                {gpu && (
                  <SettingsRow
                    label="Vulkan"
                    description="GPU acceleration through Vulkan"
                    control={<Badge variant={gpu.vulkan_ready ? "green" : "neutral"} size="sm">{gpu.vulkan_ready ? "Ready" : "Not available"}</Badge>}
                  />
                )}
              </SettingsCard>
            </>
          )}

          {/* PROVIDERS: REMOTE */}
          {remoteSpec && (
            <>
              {policyCard}
              <SettingsCard title={remoteSpec.label}>
                <SettingsRow
                  label={remoteSpec.label}
                  description={remoteSpec.blurb}
                  control={
                    <>
                      {configFor(remoteSpec.id)?.configured && <Badge variant="green" size="sm">Configured</Badge>}
                      {selectButton(remoteSpec.id)}
                    </>
                  }
                />
                <SettingsRow label="Endpoint" description="Base URL requests are sent to">
                  <input
                    aria-label={`${remoteSpec.label} endpoint`}
                    type="url"
                    value={formValue(remoteSpec, "endpoint")}
                    onChange={(e) => setForm(remoteSpec.id, { endpoint: e.target.value })}
                    className={inputClass}
                  />
                </SettingsRow>
                <SettingsRow label="Model" description="Model identifier sent with each generation request">
                  <input
                    aria-label={`${remoteSpec.label} model`}
                    type="text"
                    value={formValue(remoteSpec, "model")}
                    onChange={(e) => setForm(remoteSpec.id, { model: e.target.value })}
                    placeholder="model name"
                    className={inputClass}
                  />
                </SettingsRow>
                {remoteSpec.needsKey && (
                  <SettingsRow label="API Key" description="Stored in your operating-system keychain, never in plaintext">
                    <label htmlFor={`${remoteSpec.id}-key`} className="block text-[11px] font-mono text-ink-muted mb-1.5">
                      API Key (Redacted)
                    </label>
                    <input
                      id={`${remoteSpec.id}-key`}
                      type="password"
                      value={forms[remoteSpec.id]?.key ?? ""}
                      onChange={(e) => setForm(remoteSpec.id, { key: e.target.value })}
                      placeholder={remoteSpec.id === "openai" ? "sk-proj-..." : "API key"}
                      className={inputClass}
                    />
                  </SettingsRow>
                )}
                <div className="py-3.5">
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={
                      !formValue(remoteSpec, "endpoint").trim() ||
                      !formValue(remoteSpec, "model").trim() ||
                      (remoteSpec.needsKey && !(forms[remoteSpec.id]?.key ?? "").trim())
                    }
                    onClick={() => saveRemote(remoteSpec)}
                  >
                    {remoteSpec.needsKey ? "Save keychain credential" : "Save configuration"}
                  </Button>
                </div>
              </SettingsCard>
              {providerMessage && <p role="status" className="px-2 text-xs text-green-text">{providerMessage}</p>}
            </>
          )}

          {/* STORAGE */}
          {activeCategory === "storage" && (
            <SettingsCard title="Storage Paths & Migration">
              <SettingsRow
                label="Meeting Storage"
                description="SQLite database and media files."
                control={<PathChip path={storage?.data_root ?? "Loading…"} />}
              />
              <SettingsRow
                label="Models Storage"
                description="GGUF model weights."
                control={
                  <>
                    <PathChip path={storage?.models_root ?? "Loading…"} />
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => {
                        const target = window.prompt("Enter an empty directory for managed models");
                        if (target) void migrateModels(target).then((message) => setProviderMessage(message)).catch((error: unknown) => setProviderMessage(error instanceof Error ? error.message : "Storage migration failed."));
                      }}
                    >
                      Relocate Models…
                    </Button>
                  </>
                }
              />
              {providerMessage && (
                <div className="py-3">
                  <p role="status" className="text-xs text-green-text">{providerMessage}</p>
                </div>
              )}
            </SettingsCard>
          )}

          {/* PRIVACY */}
          {activeCategory === "privacy" && (
            <SettingsCard title="Privacy & Telemetry Audit">
              <SettingsRow
                label="Application Analytics & Tracking"
                description="Locus is sovereign desktop software designed for confidential conversations."
                control={<Badge variant="green" size="sm">Disabled (0 bytes sent)</Badge>}
              />
              <SettingsRow label="Crash Diagnostics Egress" control={<Badge variant="green" size="sm">Disabled (Local Logs Only)</Badge>} />
              <SettingsRow label="Model Weight Hash Verification" control={<Badge variant="green" size="sm">SHA-256 Enforced</Badge>} />
            </SettingsCard>
          )}
        </div>
      </div>
    </div>
  );
};

export const SettingsView: React.FC = () => {
  return (
    <ErrorBoundary fallbackTitle="Unable to load Settings">
      <SettingsContent />
    </ErrorBoundary>
  );
};
