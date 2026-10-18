import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, cleanup, within, waitFor, act } from "@testing-library/react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import * as tauri from "../lib/tauri";
import type { ModelAssetDTO, ModelDownloadEvent } from "../lib/tauri";

// Both the app shell (sidebar popover) and the Models page listen, so events go to every subscriber.
const handlers = new Set<(event: ModelDownloadEvent) => void>();
const emit = (event: ModelDownloadEvent) => handlers.forEach((handler) => handler(event));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    listModels: vi.fn(actual.listModels),
    downloadModel: vi.fn(async () => undefined),
    cancelModelDownload: vi.fn(async () => true),
    refreshModelCatalog: vi.fn(async () => ({ updated: [], unchanged: [], failed: [] })),
    onModelDownloadProgress: vi.fn(async (handler: (e: ModelDownloadEvent) => void) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    }),
  };
});

const llm = (over: Partial<ModelAssetDTO> = {}): ModelAssetDTO => ({
  id: "generation:unsloth/gemma-3-4b-it:catalog",
  catalog_id: "unsloth/gemma-3-4b-it",
  name: "gemma-3-4b-it",
  role: "llm",
  parameter_size: "4B",
  quantization: "Q4_K_M",
  memory_ram: "5.1 GB",
  memory_vram: "—",
  disk_size: "2.5 GB",
  status: "available",
  author: "unsloth",
  variants: [
    { id: "q4_k_m", quantization: "Q4_K_M", disk_size: "2.5 GB", url: "https://huggingface.co/x" },
    { id: "q8_0", quantization: "Q8_0", disk_size: "4.1 GB", url: "https://huggingface.co/y" },
  ],
  ...over,
});

const openLlmTab = async () => {
  renderApp("/models");
  fireEvent.click(await screen.findByRole("button", { name: "Summarization (llama-server)" }));
};

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ theme: "system", sidebarCollapsed: false, modelsInstalledOnly: false });
  vi.mocked(tauri.listModels).mockResolvedValue([llm()]);
  vi.mocked(tauri.downloadModel).mockClear();
  vi.mocked(tauri.cancelModelDownload).mockClear();
});
afterEach(() => {
  cleanup();
  handlers.clear();
  useUiStore.setState({ activeDownloads: {} });
  useUiStore.getState().setDownloadStatus({ active: false, modelName: "", progress: 0 });
});

describe("Model downloads", () => {
  it("the card's Download button downloads the variant the card summarises", async () => {
    await openLlmTab();
    fireEvent.click(await screen.findByRole("button", { name: "Download" }));
    expect(tauri.downloadModel).toHaveBeenCalledWith("unsloth/gemma-3-4b-it", "q4_k_m");
  });

  it("a variant pill downloads exactly that variant", async () => {
    await openLlmTab();
    fireEvent.click(await screen.findByRole("button", { name: /Show variants/ }));
    const list = screen.getByRole("list", { name: "gemma-3-4b-it variants" });
    const pill = within(list).getByRole("button", { name: "Download Q8_0" }) as HTMLButtonElement;
    expect(pill.disabled).toBe(false);
    fireEvent.click(pill);
    expect(tauri.downloadModel).toHaveBeenCalledWith("unsloth/gemma-3-4b-it", "q8_0");
  });

  it("rows without a catalog id cannot be downloaded", async () => {
    vi.mocked(tauri.listModels).mockResolvedValue([llm({ catalog_id: undefined })]);
    await openLlmTab();
    const button = (await screen.findByRole("button", { name: "Download" })) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: /Show variants/ }));
    const pill = screen.getByRole("button", { name: "Download Q8_0" }) as HTMLButtonElement;
    expect(pill.disabled).toBe(true);
  });

  it("shows live progress from events, and Cancel stops that download", async () => {
    await openLlmTab();
    await screen.findByRole("button", { name: "Download" });
    act(() => emit({ catalog_id: "unsloth/gemma-3-4b-it", display_name: "gemma-3-4b-it", variant_id: "q4_k_m", state: "running", received: 50, total: 200 }));

    const bar = await screen.findByRole("progressbar", { name: "Downloading gemma-3-4b-it" });
    expect(bar.getAttribute("aria-valuenow")).toBe("25");
    expect(screen.queryByRole("button", { name: "Download" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Cancel download of gemma-3-4b-it" }));
    expect(tauri.cancelModelDownload).toHaveBeenCalledWith("unsloth/gemma-3-4b-it");

    act(() => emit({ catalog_id: "unsloth/gemma-3-4b-it", display_name: "gemma-3-4b-it", variant_id: "q4_k_m", state: "canceled", received: 0, total: 200 }));
    expect(await screen.findByRole("button", { name: "Download" })).toBeTruthy();
  });

  it("a failed download shows the reason and offers a retry", async () => {
    await openLlmTab();
    await screen.findByRole("button", { name: "Download" });
    act(() =>
      emit({ catalog_id: "unsloth/gemma-3-4b-it", display_name: "gemma-3-4b-it", variant_id: "q4_k_m", state: "failed", received: 0, total: 200, error: "checksum mismatch" }),
    );
    expect((await screen.findByRole("alert")).textContent).toContain("checksum mismatch");
    fireEvent.click(screen.getByRole("button", { name: "Retry download" }));
    expect(tauri.downloadModel).toHaveBeenCalledTimes(1);
  });

  it("a finished download refreshes the model list", async () => {
    await openLlmTab();
    await screen.findByRole("button", { name: "Download" });
    const before = vi.mocked(tauri.listModels).mock.calls.length;
    vi.mocked(tauri.listModels).mockResolvedValue([llm({ status: "installed" })]);
    act(() => emit({ catalog_id: "unsloth/gemma-3-4b-it", display_name: "gemma-3-4b-it", variant_id: "q4_k_m", state: "done", received: 200, total: 200 }));
    await waitFor(() => expect(vi.mocked(tauri.listModels).mock.calls.length).toBeGreaterThan(before));
    expect(await screen.findByRole("button", { name: "Set Active" })).toBeTruthy();
  });

  it("feeds the sidebar Downloads popover, from any page, and empties it when the download ends", async () => {
    renderApp("/record"); // not the Models page
    const aside = await screen.findByLabelText("Primary navigation");
    fireEvent.click(within(aside).getByRole("button", { name: "Downloads" }));
    const popover = await screen.findByRole("dialog", { name: "Download progress" });
    expect(within(popover).getByText(/will appear here/i)).toBeTruthy();

    act(() => emit({ catalog_id: "whisper-small", display_name: "Whisper Small", variant_id: "standard", state: "running", received: 100, total: 400 }));
    expect(await within(popover).findByText("Whisper Small")).toBeTruthy();
    expect(within(popover).getByText("25%")).toBeTruthy();
    expect(within(popover).getByRole("progressbar", { name: "Downloading Whisper Small" }).getAttribute("aria-valuenow")).toBe("25");

    act(() => emit({ catalog_id: "whisper-small", display_name: "Whisper Small", variant_id: "standard", state: "done", received: 400, total: 400 }));
    expect(await within(popover).findByText(/will appear here/i)).toBeTruthy();
  });

  it("stacks several running downloads, each with its own progress, instead of replacing one another", async () => {
    renderApp("/record");
    const aside = await screen.findByLabelText("Primary navigation");
    const run = (id: string, name: string, received: number) =>
      act(() => emit({ catalog_id: id, display_name: name, variant_id: "x", state: "running", received, total: 100 }));
    run("a", "Model A", 10);
    run("b", "Model B", 50);
    run("a", "Model A", 20); // interleaved updates must not swap the rows
    run("b", "Model B", 60);
    fireEvent.click(within(aside).getByRole("button", { name: "Downloads" }));
    const popover = await screen.findByRole("dialog", { name: "Download progress" });

    const rows = within(popover).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]).getByText("Model A")).toBeTruthy();
    expect(within(rows[0]).getByText("20%")).toBeTruthy();
    expect(within(rows[1]).getByText("Model B")).toBeTruthy();
    expect(within(rows[1]).getByText("60%")).toBeTruthy();
  });

  it("drops only the download that ended and keeps the other one listed", async () => {
    renderApp("/record");
    const aside = await screen.findByLabelText("Primary navigation");
    act(() => emit({ catalog_id: "a", display_name: "Model A", variant_id: "x", state: "running", received: 10, total: 100 }));
    act(() => emit({ catalog_id: "b", display_name: "Model B", variant_id: "x", state: "running", received: 50, total: 100 }));
    act(() => emit({ catalog_id: "b", display_name: "Model B", variant_id: "x", state: "failed", received: 0, total: 100, error: "x" }));
    fireEvent.click(within(aside).getByRole("button", { name: "Downloads" }));
    const popover = await screen.findByRole("dialog", { name: "Download progress" });
    expect(within(popover).getAllByRole("listitem")).toHaveLength(1);
    expect(within(popover).getByText("Model A")).toBeTruthy();
    expect(within(popover).queryByText("Model B")).toBeNull();
  });

  it("asks the backend for a newer catalog when the page opens", async () => {
    await openLlmTab();
    await waitFor(() => expect(tauri.refreshModelCatalog).toHaveBeenCalled());
  });
});
