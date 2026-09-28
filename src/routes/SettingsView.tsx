import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "../components/layout/Header";
import { Button } from "../components/ui/Button";
import { Badge } from "../components/ui/Badge";
import { ErrorBoundary } from "../components/ui/ErrorBoundary";
import { useUiStore, type ThemeMode } from "../stores/uiStore";
import { checkForUpdate, configureProvider, getStorageInfo, listProviderConfigs, migrateModels, saveProviderApiKey, type ProviderConfigDTO, type StorageInfoDTO } from "../lib/tauri";
import {
  Settings,
  Palette,
  Mic,
  Key,
  HardDrive,
  Shield,
  Check,
  EyeOff,
  Sun,
  Moon,
  Monitor,
  Lock,
} from "lucide-react";

type SettingsCategory =
  | "general"
  | "appearance"
  | "recording"
  | "providers"
  | "storage"
  | "privacy";

export const SettingsContent: React.FC = () => {
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("general");
  const queryClient = useQueryClient();
  const { theme, setTheme } = useUiStore();

  const [openaiKey, setOpenaiKey] = useState("");
  const [anthropicKey, setAnthropicKey] = useState("");
  const [geminiKey, setGeminiKey] = useState("");
  const [selectedProvider, setSelectedProvider] = useState<"local" | "openai" | "anthropic" | "gemini">("local");
  const [providerMessage, setProviderMessage] = useState<string | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const { data: providerConfigs = [] } = useQuery<ProviderConfigDTO[]>({ queryKey: ["provider-configs"], queryFn: listProviderConfigs });
  const { data: storage } = useQuery<StorageInfoDTO>({ queryKey: ["storage-info"], queryFn: getStorageInfo });

  const categories = [
    { id: "general", label: "General", icon: Settings },
    { id: "appearance", label: "Appearance & Theme", icon: Palette },
    { id: "recording", label: "Recording Defaults", icon: Mic },
    { id: "providers", label: "LLM Providers", icon: Key },
    { id: "storage", label: "Storage & Relocation", icon: HardDrive },
    { id: "privacy", label: "Privacy & Sovereignty", icon: Shield },
  ] as const;

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-bg">
      <Header
        title="Settings"
        subtitle="Manage appearance, default capture preferences, provider credentials, and storage location"
      />

      <div className="flex-1 grid grid-cols-1 md:grid-cols-12 gap-0 overflow-hidden">
        {/* Category Navigation (Column 2) */}
        <div className="md:col-span-4 xl:col-span-3 border-r border-border/80 bg-surface/30 p-3 space-y-1 overflow-y-auto">
          {categories.map((cat) => {
            const Icon = cat.icon;
            const isActive = activeCategory === cat.id;
            return (
              <button
                key={cat.id}
                type="button"
                onClick={() => setActiveCategory(cat.id)}
                className={`w-full h-9 px-3 rounded-lg text-xs font-medium flex items-center gap-2.5 transition-colors text-left ${
                  isActive
                    ? "bg-green-tint text-green-text font-semibold border border-primary/20"
                    : "text-ink-muted hover:text-ink hover:bg-surface"
                }`}
              >
                <Icon className={`w-4 h-4 ${isActive ? "text-primary" : "text-ink-muted"}`} />
                <span>{cat.label}</span>
              </button>
            );
          })}
        </div>

        {/* Category Content Panel (Column 3) */}
        <div className="md:col-span-8 xl:col-span-9 p-8 overflow-y-auto bg-surface-elevated space-y-8 max-w-2xl">
          {/* GENERAL */}
          {activeCategory === "general" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-sm font-semibold text-ink">General</h2>
                <p className="text-xs text-ink-muted mt-1">Locus keeps capture local by default and never enables a remote destination implicitly.</p>
              </div>
              <div className="rounded-lg border border-border bg-surface p-4 text-xs space-y-3">
                <div className="flex items-center justify-between"><span className="text-ink-muted">Configured providers</span><span className="font-semibold text-ink">{providerConfigs.filter((config) => config.configured).length}</span></div>
                <div className="flex items-center justify-between border-t border-border pt-3"><span className="text-ink-muted">Automatic telemetry</span><Badge variant="green" size="sm">Disabled</Badge></div>
                <div className="flex items-center justify-between border-t border-border pt-3 gap-3"><span className="text-ink-muted">Updates</span><Button variant="secondary" size="sm" onClick={() => void checkForUpdate().then((result) => setUpdateMessage(result.available_version ? `Version ${result.available_version} is available on the stable channel.` : "Locus is up to date on the stable channel.")).catch((error: unknown) => setUpdateMessage(error instanceof Error ? error.message : "Update check failed."))}>Check stable channel</Button></div>
                {updateMessage && <p role="status" className="text-green-text">{updateMessage}</p>}
              </div>
            </div>
          )}
          {/* APPEARANCE */}
          {activeCategory === "appearance" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-sm font-semibold text-ink">Color Theme</h2>
                <p className="text-xs text-ink-muted mt-1">
                  Choose between Architectural Paper Studio (Light), Obsidian Slate (Dark), or synchronize with your operating system preferences.
                </p>
              </div>

              <div className="grid grid-cols-3 gap-3">
                {[
                  { id: "system", label: "System Sync", icon: Monitor },
                  { id: "light", label: "Paper Studio (Light)", icon: Sun },
                  { id: "dark", label: "Obsidian Slate (Dark)", icon: Moon },
                ].map((opt) => {
                  const Icon = opt.icon;
                  const isSel = theme === opt.id;
                  return (
                    <button
                      key={opt.id}
                      type="button"
                      onClick={() => setTheme(opt.id as ThemeMode)}
                      className={`p-4 rounded-lg border text-left transition-all ${
                        isSel
                          ? "border-primary bg-green-tint/30 ring-1 ring-primary/40 font-semibold"
                          : "border-border bg-surface hover:bg-surface-sunken text-ink-muted"
                      }`}
                    >
                      <Icon className={`w-5 h-5 mb-2 ${isSel ? "text-primary" : "text-ink-muted"}`} />
                      <span className="text-xs text-ink block">{opt.label}</span>
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {/* RECORDING DEFAULTS */}
          {activeCategory === "recording" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-sm font-semibold text-ink">Capture Defaults</h2>
                <p className="text-xs text-ink-muted mt-1">
                  Standard capture configuration applied when launching new sessions.
                </p>
              </div>

              <div className="space-y-3 bg-surface-sunken p-4 rounded-lg border border-border text-xs">
                <div className="flex items-center justify-between">
                  <div>
                    <span className="font-semibold text-ink block">Default System Audio</span>
                    <span className="text-ink-muted">Always pre-select System Audio on session launch</span>
                  </div>
                  <input type="checkbox" defaultChecked className="rounded accent-primary" />
                </div>
                <div className="flex items-center justify-between pt-2 border-t border-border/60">
                  <div>
                    <span className="font-semibold text-ink block">Default Screen Video</span>
                    <span className="text-ink-muted">Enable automatic slide extraction via OpenCV differencing</span>
                  </div>
                  <input type="checkbox" defaultChecked className="rounded accent-primary" />
                </div>
                <div className="flex items-center justify-between pt-2 border-t border-border/60">
                  <div>
                    <span className="font-semibold text-ink block">3-Hour Advisory Banner</span>
                    <span className="text-ink-muted">Show non-blocking advisory notification without interrupting recording</span>
                  </div>
                  <input type="checkbox" defaultChecked className="rounded accent-primary" />
                </div>
              </div>
            </div>
          )}

          {/* LLM PROVIDERS */}
          {activeCategory === "providers" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-sm font-semibold text-ink">AI Provider Configuration & Authorization</h2>
                <p className="text-xs text-ink-muted mt-1">
                  Configuring credentials makes remote APIs available, but Locus will never send meeting content until you explicitly select that provider as your authorized destination.
                </p>
              </div>

              {/* Destination Authorization Model */}
              <div className="p-4 rounded-lg bg-green-tint/40 border border-green-text/20 text-xs text-ink space-y-2">
                <div className="flex items-center gap-2 font-semibold text-green-text">
                  <Lock className="w-4 h-4 text-primary" />
                  <span>Sovereign Provider Policy (Zero Silent Remote Failover)</span>
                </div>
                <p className="leading-relaxed">
                  Active Selected Provider: <strong>{selectedProvider === "local" ? "Local Llama 3.2 3B (Offline)" : selectedProvider}</strong>. Credentials are saved exclusively in your OS Keychain (Apple Keychain / Windows Credential Manager / Secret Service).
                </p>
              </div>

              <div className="space-y-4">
                {/* Local llama-server */}
                <div className="p-4 rounded-lg border border-border bg-surface flex items-center justify-between">
                  <div>
                    <h3 className="text-xs font-semibold text-ink">Local llama-server</h3>
                    <p className="text-[11px] text-ink-muted mt-0.5">Private loopback inference, zero data leaves machine</p>
                  </div>
                  <Button
                    variant={selectedProvider === "local" ? "primary" : "secondary"}
                    size="sm"
                    onClick={() => setSelectedProvider("local")}
                  >
                    {selectedProvider === "local" ? "Selected (Active)" : "Select"}
                  </Button>
                </div>

                {/* OpenAI */}
                <div className="p-4 rounded-lg border border-border bg-surface space-y-3">
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-xs font-semibold text-ink">OpenAI (ChatGPT / GPT-4o)</h3>
                      <p className="text-[11px] text-ink-muted mt-0.5">Cloud API client, requires user API key</p>
                    </div>
                    <Button
                      variant={selectedProvider === "openai" ? "primary" : "secondary"}
                      size="sm"
                      onClick={() => setSelectedProvider("openai")}
                    >
                      {selectedProvider === "openai" ? "Selected (Active)" : "Select"}
                    </Button>
                  </div>
                  <div>
                    <label className="block text-[11px] font-mono text-ink-muted mb-1">
                      API Key (Redacted)
                    </label>
                    <input
                      type="password"
                      value={openaiKey}
                      onChange={(e) => setOpenaiKey(e.target.value)}
                      placeholder="sk-proj-..."
                      className="w-full h-8 px-2.5 rounded border border-border bg-surface-sunken text-xs font-mono text-ink"
                    />
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={!openaiKey.trim()}
                    onClick={() => {
                      void configureProvider("openai", "gpt-4o-mini", "https://api.openai.com", true)
                        .then(() => saveProviderApiKey("openai", openaiKey))
                        .then(() => { setProviderMessage("OpenAI is configured in the operating-system keychain."); void queryClient.invalidateQueries({ queryKey: ["provider-configs"] }); })
                        .catch((error: unknown) => setProviderMessage(error instanceof Error ? error.message : "Provider configuration failed."));
                    }}
                  >
                    Save keychain credential
                  </Button>
                </div>
                {providerMessage && <p role="status" className="text-xs text-green-text">{providerMessage}</p>}
              </div>
            </div>
          )}

          {/* STORAGE */}
          {activeCategory === "storage" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-sm font-semibold text-ink">Storage Paths & Migration</h2>
                <p className="text-xs text-ink-muted mt-1">
                  Manage disk location for SQLite database, media files, and GGUF model weights.
                </p>
              </div>

              <div className="p-4 rounded-lg border border-border bg-surface space-y-3 text-xs">
                <div>
                  <span className="text-ink-muted block text-[11px] uppercase font-mono">Current Meeting Storage Path</span>
                  <span className="font-mono text-ink block mt-1 bg-surface-sunken p-2 rounded border border-border">
                    {storage?.data_root ?? "Loading…"}
                  </span>
                </div>
                <div>
                  <span className="text-ink-muted block text-[11px] uppercase font-mono">Models Storage Path</span>
                  <span className="font-mono text-ink block mt-1 bg-surface-sunken p-2 rounded border border-border">
                    {storage?.models_root ?? "Loading…"}
                  </span>
                </div>
                <div className="pt-2 flex justify-end">
                  <Button variant="secondary" size="sm" onClick={() => {
                    const target = window.prompt("Enter an empty directory for managed models");
                    if (target) void migrateModels(target).then((message) => setProviderMessage(message)).catch((error: unknown) => setProviderMessage(error instanceof Error ? error.message : "Storage migration failed."));
                  }}>
                    Relocate Models…
                  </Button>
                </div>
              </div>
            </div>
          )}

          {/* PRIVACY */}
          {activeCategory === "privacy" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-sm font-semibold text-ink">Privacy & Telemetry Audit</h2>
                <p className="text-xs text-ink-muted mt-1">
                  Locus is sovereign desktop software designed for confidential conversations.
                </p>
              </div>

              <div className="space-y-3 text-xs">
                <div className="p-3 rounded border border-border bg-surface flex items-center justify-between">
                  <span className="font-medium text-ink">Application Analytics & Tracking</span>
                  <Badge variant="green" size="sm">Disabled (0 bytes sent)</Badge>
                </div>
                <div className="p-3 rounded border border-border bg-surface flex items-center justify-between">
                  <span className="font-medium text-ink">Crash Diagnostics Egress</span>
                  <Badge variant="green" size="sm">Disabled (Local Logs Only)</Badge>
                </div>
                <div className="p-3 rounded border border-border bg-surface flex items-center justify-between">
                  <span className="font-medium text-ink">Model Weight Hash Verification</span>
                  <Badge variant="green" size="sm">SHA-256 Enforced</Badge>
                </div>
              </div>
            </div>
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
