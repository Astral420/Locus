import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, cleanup, within, waitFor, act } from "@testing-library/react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import * as tauri from "../lib/tauri";
import type { ModelAssetDTO, ModelDownloadEvent } from "../lib/tauri";

// The app shell and the Models page both listen for download progress.
const handlers = new Set<(event: ModelDownloadEvent) => void>();
const emit = (event: ModelDownloadEvent) => handlers.forEach((handler) => handler(event));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    listModels: vi.fn(actual.listModels),
    uninstallModel: vi.fn(),
    refreshModelCatalog: vi.fn(async () => ({ updated: [], unchanged: [], failed: [] })),
    onModelDownloadProgress: vi.fn(async (handler: (e: ModelDownloadEvent) => void) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    }),
  };
});

const whisperModel = (over: Partial<ModelAssetDTO> = {}): ModelAssetDTO => ({
  id: "whisper:small:standard",
  catalog_id: "whisper-small",
  name: "Whisper Small",
  role: "whisper",
  parameter_size: "244M",
  quantization: "Full precision",
  memory_ram: "1.0 GB",
  memory_vram: "1.0 GB",
  disk_size: "466 MiB",
  status: "active",
  is_default: true,
  variants: [
    {
      id: "standard",
      quantization: "Full precision",
      disk_size: "466 MiB",
      installed: true,
      active: true,
      select_id: "whisper:small:standard",
      removable: true,
    },
    { id: "q5_1", quantization: "Q5_1", disk_size: "190 MiB", installed: false, active: false },
  ],
  ...over,
});

const llmModel = (over: Partial<ModelAssetDTO> = {}): ModelAssetDTO => ({
  id: "generation:a/one:q4_k_m",
  catalog_id: "a/one",
  name: "Gemma 3 4B",
  role: "llm",
  parameter_size: "4B",
  quantization: "Q4_K_M",
  memory_ram: "5.1 GB",
  memory_vram: "—",
  disk_size: "2.5 GB",
  status: "active",
  variants: [
    {
      id: "q4_k_m",
      quantization: "Q4_K_M",
      disk_size: "2.5 GB",
      installed: true,
      active: true,
      select_id: "generation:a/one:q4_k_m-01234567",
      removable: true,
    },
  ],
  ...over,
});

const embeddingModel = (over: Partial<ModelAssetDTO> = {}): ModelAssetDTO => ({
  id: "embedding:a/two:q8_0",
  catalog_id: "a/two",
  name: "MiniLM",
  role: "embedding",
  parameter_size: "33M",
  quantization: "Q8_0",
  memory_ram: "500 MB",
  memory_vram: "500 MB",
  disk_size: "133 MiB",
  status: "active",
  variants: [
    {
      id: "q8_0",
      quantization: "Q8_0",
      disk_size: "133 MiB",
      installed: true,
      active: true,
      select_id: "embedding:a/two:q8_0-01234567",
      removable: true,
    },
  ],
  ...over,
});

const result = (removed: string[], freed = 466_000_000) => ({ removed, freed_bytes: freed, new_selection: null });

const expandVariants = async () =>
  fireEvent.click(await screen.findByRole("button", { name: /Show variants/ }));
const dialog = () => screen.getByRole("dialog");

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ theme: "system", sidebarCollapsed: false, modelsInstalledOnly: false });
  vi.mocked(tauri.listModels).mockResolvedValue([whisperModel()]);
  vi.mocked(tauri.uninstallModel).mockReset();
  vi.mocked(tauri.uninstallModel).mockResolvedValue(result(["standard"]));
});
afterEach(() => {
  cleanup();
  handlers.clear();
});

describe("Model uninstall", () => {
  it("offers Remove only where Locus owns the file, and labels the rest as managed", async () => {
    vi.mocked(tauri.listModels).mockResolvedValue([
      whisperModel({
        variants: [
          {
            id: "standard",
            quantization: "Full precision",
            disk_size: "466 MiB",
            installed: true,
            active: true,
            select_id: "whisper:small:standard",
            removable: true,
          },
          {
            id: "q5_1",
            quantization: "Q5_1",
            disk_size: "190 MiB",
            installed: true,
            active: false,
            select_id: "whisper:small:q5_1",
            removable: false,
          },
        ],
      }),
    ]);
    renderApp("/models");
    await screen.findByText("Whisper Small");
    await expandVariants();

    expect(screen.getByRole("button", { name: "Remove Full precision" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Remove Q5_1" })).toBeNull();
    expect(screen.getByText("Managed outside Locus")).toBeTruthy();
    expect(screen.queryByText("Included with the app")).toBeNull();
  });

  it("offers Uninstall for one removable variant and Uninstall all for several", async () => {
    renderApp("/models");
    expect(await screen.findByRole("button", { name: "Uninstall" })).toBeTruthy();

    cleanup();
    vi.mocked(tauri.listModels).mockResolvedValue([
      whisperModel({
        variants: [
          {
            id: "standard",
            quantization: "Full precision",
            disk_size: "466 MiB",
            installed: true,
            active: true,
            select_id: "whisper:small:standard",
            removable: true,
          },
          {
            id: "q5_1",
            quantization: "Q5_1",
            disk_size: "190 MiB",
            installed: true,
            active: false,
            select_id: "whisper:small:q5_1",
            removable: true,
          },
        ],
      }),
    ]);
    renderApp("/models");
    expect(await screen.findByRole("button", { name: "Uninstall all (2)" })).toBeTruthy();
  });

  it("opens a confirmation, and Cancel closes it without removing anything", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    await expandVariants();
    fireEvent.click(screen.getByRole("button", { name: "Remove Full precision" }));

    expect(screen.getByText("Remove Whisper Small · Full precision?")).toBeTruthy();
    expect(screen.getByText(/This deletes 1 file from this computer/)).toBeTruthy();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(tauri.uninstallModel).not.toHaveBeenCalled();
  });

  it("confirming calls the command, reloads the list and reports what was freed", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    await expandVariants();
    const reloads = vi.mocked(tauri.listModels).mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: "Remove Full precision" }));
    fireEvent.click(within(dialog()).getByRole("button", { name: "Remove" }));

    await waitFor(() => expect(tauri.uninstallModel).toHaveBeenCalledWith("whisper-small", "standard"));
    expect((await screen.findByRole("status")).textContent).toContain("Removed Whisper Small (Full precision). Freed 466 MB.");
    await waitFor(() => expect(vi.mocked(tauri.listModels).mock.calls.length).toBeGreaterThan(reloads));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("uninstalling the whole model passes no variant id", async () => {
    renderApp("/models");
    fireEvent.click(await screen.findByRole("button", { name: "Uninstall" }));
    expect(screen.getByText("Uninstall Whisper Small?")).toBeTruthy();

    fireEvent.click(within(dialog()).getByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(tauri.uninstallModel).toHaveBeenCalledWith("whisper-small", undefined));
  });

  it("warns before the last whisper variant goes, including the automatic switch", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    await expandVariants();
    fireEvent.click(screen.getByRole("button", { name: "Remove Full precision" }));

    expect(
      screen.getByText(/Locus will switch to another installed quantization of this model if there is one./),
    ).toBeTruthy();
    expect(
      screen.getByText(/Transcription is unavailable until a Whisper model is installed. Recordings are still saved./),
    ).toBeTruthy();
  });

  it("keeps the existing index when an embedding model is removed", async () => {
    vi.mocked(tauri.listModels).mockResolvedValue([embeddingModel()]);
    renderApp("/models");
    fireEvent.click(await screen.findByRole("button", { name: "Embedding (RAG)" }));
    await screen.findByText("MiniLM");
    await expandVariants();
    fireEvent.click(screen.getByRole("button", { name: "Remove Q8_0" }));

    expect(
      screen.getByText(
        /Knowledge search stops working until an embedding model is selected. Your existing index is kept; installing the same model again restores it./,
      ),
    ).toBeTruthy();
  });

  it("a refused removal keeps the dialog open and shows the reason", async () => {
    vi.mocked(tauri.uninstallModel).mockRejectedValue(new Error("a transcription job is running"));
    renderApp("/models");
    await screen.findByText("Whisper Small");
    await expandVariants();
    const reloads = vi.mocked(tauri.listModels).mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: "Remove Full precision" }));
    fireEvent.click(within(dialog()).getByRole("button", { name: "Remove" }));

    expect((await screen.findByRole("alert")).textContent).toContain("a transcription job is running");
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(vi.mocked(tauri.listModels).mock.calls.length).toBe(reloads);
  });

  it("disables removal while that model is downloading", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    await expandVariants();
    expect((screen.getByRole("button", { name: "Remove Full precision" }) as HTMLButtonElement).disabled).toBe(false);

    act(() =>
      emit({
        catalog_id: "whisper-small",
        display_name: "Whisper Small",
        variant_id: "standard",
        state: "running",
        received: 1,
        total: 2,
      }),
    );

    await waitFor(() =>
      expect((screen.getByRole("button", { name: "Remove Full precision" }) as HTMLButtonElement).disabled).toBe(true),
    );
    expect((screen.getByRole("button", { name: "Uninstall" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("names the language tab, explains each tab and drops the old label", async () => {
    vi.mocked(tauri.listModels).mockResolvedValue([whisperModel(), llmModel()]);
    renderApp("/models");
    const tab = await screen.findByRole("button", { name: "Local Language Model" });

    expect(screen.queryByText("Summarization (llama-server)")).toBeNull();
    expect(screen.getByTestId("tab-purpose").textContent).toBe("Turns recordings into transcripts.");

    fireEvent.click(tab);
    expect(screen.getByTestId("tab-purpose").textContent).toBe("Writes meeting summaries and answers questions in chat.");

    fireEvent.click(screen.getByRole("button", { name: "Embedding (RAG)" }));
    expect(screen.getByTestId("tab-purpose").textContent).toBe("Powers knowledge search.");
  });

  it("says no language models are installed yet", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    fireEvent.click(screen.getByRole("switch", { name: "Show installed models only" }));
    fireEvent.click(screen.getByRole("button", { name: "Local Language Model" }));

    expect(await screen.findByText(/No language models are installed yet/)).toBeTruthy();
  });
});
