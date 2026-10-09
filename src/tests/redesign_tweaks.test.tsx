import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, cleanup, within, waitFor } from "@testing-library/react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import * as tauri from "../lib/tauri";

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    configureProvider: vi.fn(actual.configureProvider),
    saveProviderApiKey: vi.fn(actual.saveProviderApiKey),
  };
});

const NOTICE_KEY = "locus_core_engine_notice_dismissed";
const h1 = (name: string | RegExp) => screen.findByRole("heading", { level: 1, name });
const PAGES: [string, string | RegExp][] = [
  ["/", "Meetings"],
  ["/record", "Capture Hub"],
  ["/meeting/m-01", /Q3 Architecture/],
  ["/knowledge", "Knowledge Base"],
  ["/models", "Model Manager"],
  ["/settings", "Settings"],
];

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ sidebarCollapsed: false, coreEngineNoticeDismissed: false });
});
afterEach(() => cleanup());

describe("Tweak: no Locus name and no engine status in the sidebar", () => {
  it("shows neither the wordmark nor the offline/online indicator (shown and hidden)", async () => {
    renderApp("/");
    const aside = await screen.findByLabelText("Primary navigation");
    expect(within(aside).queryByText("Locus")).toBeNull();
    expect(within(aside).queryByText(/Offline/i)).toBeNull();
    expect(within(aside).queryByText(/Local Engine/i)).toBeNull();
    expect(aside.textContent).not.toMatch(/engine|online|offline/i);

    fireEvent.click(within(aside).getByRole("button", { name: "Hide sidebar" }));
    expect(document.body.textContent).not.toMatch(/Offline · Local Engine/);
    expect(screen.queryByText("Locus")).toBeNull();
  });
});

describe("Tweak: no New Record button anywhere", () => {
  it.each(PAGES)("%s has no New Record button", async (path, heading) => {
    renderApp(path);
    await h1(heading);
    expect(screen.queryByText("New Record")).toBeNull();
    expect(screen.queryByRole("button", { name: /new record|start a new recording/i })).toBeNull();
  });
});

describe("Tweak: Model Manager", () => {
  it("has no Import Local GGUF button and no inference-backend indicator", async () => {
    renderApp("/models");
    await h1("Model Manager");
    await screen.findByText("Whisper Small (Bundled)");
    expect(screen.queryByText(/Import Local GGUF/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /import/i })).toBeNull();
    expect(screen.queryByText(/Inference backend/i)).toBeNull();
    expect(screen.queryByText(/native GPU runtime/i)).toBeNull();
  });
});

describe("Tweak: Offline-Ready Core Engine notice is first-run and dismissible for good", () => {
  it("shows on first visit, closes via the dismiss button and persists the choice", async () => {
    renderApp("/models");
    await h1("Model Manager");
    expect(screen.getByRole("region", { name: "Offline-Ready Core Engine" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Dismiss notice" }));
    expect(screen.queryByRole("region", { name: "Offline-Ready Core Engine" })).toBeNull();
    expect(localStorage.getItem(NOTICE_KEY)).toBe("true");
    expect(useUiStore.getState().coreEngineNoticeDismissed).toBe(true);
  });

  it("does not come back when navigating away and returning", async () => {
    renderApp("/models");
    await h1("Model Manager");
    fireEvent.click(screen.getByRole("button", { name: "Dismiss notice" }));

    const aside = screen.getByLabelText("Primary navigation");
    fireEvent.click(within(aside).getByRole("link", { name: "Settings" }));
    await h1("Settings");
    fireEvent.click(within(aside).getByRole("link", { name: "Model Manager" }));
    await h1("Model Manager");
    expect(screen.queryByRole("region", { name: "Offline-Ready Core Engine" })).toBeNull();
  });

  it("stays dismissed across an app restart (state is rebuilt from storage)", async () => {
    localStorage.setItem(NOTICE_KEY, "true");
    vi.resetModules();
    const fresh = await import("../stores/uiStore");
    expect(fresh.useUiStore.getState().coreEngineNoticeDismissed).toBe(true);

    localStorage.clear();
    vi.resetModules();
    const firstInstall = await import("../stores/uiStore");
    expect(firstInstall.useUiStore.getState().coreEngineNoticeDismissed).toBe(false);
  });
});

describe("Tweak: Settings → Providers (Local / Remote)", () => {
  it("groups providers under Providers with Local and Remote sub-sections", async () => {
    renderApp("/settings");
    await h1("Settings");
    const nav = screen.getByRole("navigation", { name: "Settings categories" });

    expect(within(nav).getByText("Providers")).toBeTruthy();
    expect(within(nav).queryByText("AI Providers")).toBeNull();
    expect(within(nav).queryByRole("button", { name: "LLM Providers" })).toBeNull();
    expect(within(nav).getByText("Local")).toBeTruthy();
    expect(within(nav).getByText("Remote")).toBeTruthy();
    for (const name of ["Llama.cpp", "Ollama", "OpenAI", "Anthropic", "Gemini"]) {
      expect(within(nav).getByRole("button", { name })).toBeTruthy();
    }
  });

  it("Llama.cpp carries the inference backend indicator that left the Model Manager", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Llama.cpp" }));

    expect(screen.getByRole("heading", { name: "Inference Backend" })).toBeTruthy();
    expect(await screen.findByText("Backend: CPU")).toBeTruthy();
    expect(screen.getByText(/Browser preview has no native GPU runtime/)).toBeTruthy();
    expect(screen.getByText("Architecture")).toBeTruthy();

    // Selecting it updates the authorization summary.
    expect(screen.getByText("Llama.cpp (local, offline)")).toBeTruthy();
    expect(screen.getByText("Selected (Active)")).toBeTruthy();
  });

  it("Ollama needs an endpoint and a model but no API key", async () => {
    vi.mocked(tauri.configureProvider).mockResolvedValueOnce(undefined);
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Ollama" }));

    expect(screen.queryByLabelText("API Key (Redacted)")).toBeNull();
    expect((screen.getByLabelText("Ollama endpoint") as HTMLInputElement).value).toBe("http://127.0.0.1:11434");
    const save = screen.getByRole("button", { name: "Save configuration" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true); // model still blank

    fireEvent.change(screen.getByLabelText("Ollama model"), { target: { value: "llama3.2" } });
    expect(save.disabled).toBe(false);
    fireEvent.click(save);

    await waitFor(() => expect(tauri.configureProvider).toHaveBeenCalledWith("ollama", "llama3.2", "http://127.0.0.1:11434", true));
    expect(tauri.saveProviderApiKey).not.toHaveBeenCalledWith("ollama", expect.anything());
    expect(await screen.findByText("Ollama is configured.")).toBeTruthy();
  });

  it.each([
    ["Anthropic", "anthropic", "https://api.anthropic.com"],
    ["Gemini", "gemini", "https://generativelanguage.googleapis.com/v1beta"],
  ])("%s requires a model and an API key and stores the key in the keychain", async (label, id, endpoint) => {
    vi.mocked(tauri.configureProvider).mockResolvedValueOnce(undefined);
    vi.mocked(tauri.saveProviderApiKey).mockResolvedValueOnce(undefined);
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: label }));

    expect((screen.getByLabelText(`${label} endpoint`) as HTMLInputElement).value).toBe(endpoint);
    const save = screen.getByRole("button", { name: "Save keychain credential" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText(`${label} model`), { target: { value: "some-model" } });
    expect(save.disabled).toBe(true); // key still missing
    fireEvent.change(screen.getByLabelText("API Key (Redacted)"), { target: { value: "key-123" } });
    expect(save.disabled).toBe(false);
    fireEvent.click(save);

    await waitFor(() => expect(tauri.configureProvider).toHaveBeenCalledWith(id, "some-model", endpoint, true));
    await waitFor(() => expect(tauri.saveProviderApiKey).toHaveBeenCalledWith(id, "key-123"));
    expect(await screen.findByText(`${label} is configured in the operating-system keychain.`)).toBeTruthy();
  });

  it("keeps each provider's draft separate while switching pages", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Anthropic" }));
    fireEvent.change(screen.getByLabelText("Anthropic model"), { target: { value: "draft-model" } });
    fireEvent.click(screen.getByRole("button", { name: "Gemini" }));
    expect((screen.getByLabelText("Gemini model") as HTMLInputElement).value).toBe("");
    fireEvent.click(screen.getByRole("button", { name: "Anthropic" }));
    expect((screen.getByLabelText("Anthropic model") as HTMLInputElement).value).toBe("draft-model");
  });
});
