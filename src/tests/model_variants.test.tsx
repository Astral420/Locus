import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, cleanup, within, waitFor } from "@testing-library/react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import * as tauri from "../lib/tauri";
import type { ModelAssetDTO } from "../lib/tauri";

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    listModels: vi.fn(actual.listModels),
    selectModel: vi.fn(async () => undefined),
    downloadModel: vi.fn(async () => undefined),
    refreshModelCatalog: vi.fn(async () => ({ updated: [], unchanged: [], failed: [] })),
    onModelDownloadProgress: vi.fn(async () => () => undefined),
  };
});

const whisper = (over: Partial<ModelAssetDTO> = {}): ModelAssetDTO => ({
  id: "whisper:small:standard",
  catalog_id: "whisper-small",
  name: "Whisper Small",
  role: "whisper",
  parameter_size: "244M",
  quantization: "Full precision",
  memory_ram: "1.0 GB",
  memory_vram: "1.0 GB",
  disk_size: "466 MiB",
  status: "installed",
  is_default: true,
  variants: [
    { id: "standard", quantization: "Full precision", disk_size: "466 MiB", installed: true, active: true, select_id: "whisper:small:standard" },
    { id: "q5_1", quantization: "Q5_1", disk_size: "190 MiB", installed: true, select_id: "whisper:small:q5_1" },
    { id: "q8_0", quantization: "Q8_0", disk_size: "300 MiB" },
  ],
  ...over,
});

const card = (name: string) => screen.getByRole("article", { name });

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ modelsInstalledOnly: false });
  vi.mocked(tauri.listModels).mockResolvedValue([whisper()]);
  vi.mocked(tauri.selectModel).mockClear();
  vi.mocked(tauri.downloadModel).mockClear();
});

afterEach(() => {
  cleanup();
});

describe("Model variants", () => {
  it("shows the Default tag for the default model and never says Bundled", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    expect(within(card("Whisper Small")).getByText("Default")).toBeTruthy();
    expect(screen.queryByText(/Bundled/i)).toBeNull();
  });

  it("reads 'Using Q5_1' when the headline variant is installed, and the plain label when not", async () => {
    vi.mocked(tauri.listModels).mockResolvedValue([whisper({ quantization: "Q5_1" })]);
    renderApp("/models");
    await screen.findByText("Whisper Small");
    expect(within(card("Whisper Small")).getByText("Using Q5_1")).toBeTruthy();

    cleanup();
    vi.mocked(tauri.listModels).mockResolvedValue([whisper({ status: "available", is_default: false, quantization: "Q4_K_M" })]);
    renderApp("/models");
    await screen.findByText("Whisper Small");
    expect(within(card("Whisper Small")).getByText("Q4_K_M")).toBeTruthy();
    expect(screen.queryByText("Using Q4_K_M")).toBeNull();
  });

  it("shows enabled variant controls for an installed model: Set active, Active chip and Download", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    fireEvent.click(within(card("Whisper Small")).getByRole("button", { name: /Show variants/ }));
    const list = within(card("Whisper Small")).getByRole("list", { name: "Whisper Small variants" });

    // active → non-interactive chip, no button
    expect(within(list).getByText("Active")).toBeTruthy();
    expect(within(list).queryByRole("button", { name: "Set Full precision active" })).toBeNull();

    // installed → Set active calls selectModel(variant.select_id, "whisper")
    fireEvent.click(within(list).getByRole("button", { name: "Set Q5_1 active" }));
    await waitFor(() => expect(tauri.selectModel).toHaveBeenCalledWith("whisper:small:q5_1", "whisper"));

    // not installed → Download calls downloadModel(catalog_id, variant id)
    fireEvent.click(within(list).getByRole("button", { name: "Download Q8_0" }));
    await waitFor(() => expect(tauri.downloadModel).toHaveBeenCalledWith("whisper-small", "q8_0"));
  });

  it("drives the card button from status: installed → Set active, available → Download, active → none", async () => {
    vi.mocked(tauri.listModels).mockResolvedValue([
      whisper({ id: "whisper:small:q5_1", name: "Installed Small", quantization: "Q5_1" }),
      whisper({ id: "whisper:base:catalog", name: "Available Base", status: "available", is_default: false, quantization: "Q4_K_M" }),
      whisper({ id: "whisper:tiny:standard", name: "Active Tiny", status: "active", is_default: false, quantization: "Full precision" }),
    ]);
    renderApp("/models");
    await screen.findByText("Installed Small");

    fireEvent.click(within(card("Installed Small")).getByRole("button", { name: "Set active" }));
    await waitFor(() => expect(tauri.selectModel).toHaveBeenCalledWith("whisper:small:q5_1", "whisper"));

    expect(within(card("Available Base")).getByRole("button", { name: "Download" })).toBeTruthy();
    expect(within(card("Active Tiny")).queryByRole("button", { name: "Set active" })).toBeNull();
    expect(within(card("Active Tiny")).queryByRole("button", { name: "Download" })).toBeNull();
  });

  it("reloads the model list after a selection is stored", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    const before = vi.mocked(tauri.listModels).mock.calls.length;
    fireEvent.click(within(card("Whisper Small")).getByRole("button", { name: "Set active" }));
    await waitFor(() => expect(vi.mocked(tauri.listModels).mock.calls.length).toBeGreaterThan(before));
  });

  it("keeps the Offline-Ready notice and no longer claims a bundled Whisper", async () => {
    renderApp("/models");
    await screen.findByText("Whisper Small");
    const notice = screen.getByRole("region", { name: "Offline-Ready Core Engine" });
    expect(notice.textContent).not.toMatch(/bundled/i);
    expect(notice.textContent).toMatch(/Whisper Small/);
  });
});
