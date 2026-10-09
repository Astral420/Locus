import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { screen, fireEvent, cleanup, within, waitFor, act } from "@testing-library/react";
import { renderApp } from "./renderApp";
import { useUiStore } from "../stores/uiStore";
import { useRecordingStore } from "../stores/recordingStore";
import * as tauri from "../lib/tauri";

// Wrap the mock-backed IPC layer so tests can observe calls while keeping the real mock behaviour.
vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    listModels: vi.fn(actual.listModels),
    selectModel: vi.fn(actual.selectModel),
    configureProvider: vi.fn(actual.configureProvider),
    saveProviderApiKey: vi.fn(actual.saveProviderApiKey),
    sendKnowledgeMessage: vi.fn(actual.sendKnowledgeMessage),
  };
});

const actualTauri = await vi.importActual<typeof import("../lib/tauri")>("../lib/tauri");
const h1 = (name: string | RegExp) => screen.findByRole("heading", { level: 1, name });
const MEETING_TITLE = "Q3 Architecture & Offline RAG Sync";

beforeEach(() => {
  localStorage.clear();
  useUiStore.setState({ theme: "system", sidebarCollapsed: false });
  useRecordingStore.setState({
    state: "idle",
    elapsed_seconds: 0,
    meeting_title: "Untitled Conversation",
    meeting_type: "auto",
    selected_sources: ["system_audio", "screen"],
    only_me_mic: false,
    reason: null,
    warning: false,
  });
});
afterEach(() => {
  cleanup();
  vi.mocked(tauri.listModels).mockImplementation(actualTauri.listModels);
  vi.useRealTimers();
});

describe("Integration: Record flow (composer layout)", () => {
  it("runs idle → recording → paused → resumed → stopped and returns to Meetings", async () => {
    renderApp("/record");
    expect(await screen.findByText("Ready to record?")).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Session Title"), { target: { value: "Weekly sync" } });
    fireEvent.click(screen.getByRole("button", { name: "Start Recording" }));

    expect(await screen.findByText("Recording Live")).toBeTruthy();
    expect(useRecordingStore.getState().meeting_title).toBe("Weekly sync");
    expect(screen.getByText("REC")).toBeTruthy(); // header pill
    expect(screen.getByText("Weekly sync")).toBeTruthy(); // title echoed in the live HUD

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(await screen.findByText("Capture Paused")).toBeTruthy();
    expect(screen.getByText("PAUSED")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(await screen.findByText("Recording Live")).toBeTruthy();

    // Elapsed time always renders as whole seconds in both the HUD and the header pill.
    act(() => useRecordingStore.setState({ elapsed_seconds: 5.617268483 }));
    await waitFor(() => expect(screen.getAllByText("00:05").length).toBeGreaterThanOrEqual(2));
    expect(screen.queryByText(/\d\.\d{3}/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Stop & Process/ }));
    expect(await h1("Meetings")).toBeTruthy();
    expect(useRecordingStore.getState().state).not.toBe("recording");
  });

  it("toggles capture sources, classification and the only-me-mic switch in the store", async () => {
    renderApp("/record");
    await screen.findByText("Ready to record?");

    const mic = screen.getByRole("button", { name: /Microphone/ });
    expect(mic.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(mic);
    expect(mic.getAttribute("aria-pressed")).toBe("true");
    expect(useRecordingStore.getState().selected_sources).toContain("microphone");

    fireEvent.click(screen.getByRole("button", { name: "Lecture" }));
    expect(useRecordingStore.getState().meeting_type).toBe("lecture");

    const onlyMe = screen.getByRole("switch", { name: "Only me on this microphone" });
    expect(onlyMe.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(onlyMe);
    expect(useRecordingStore.getState().only_me_mic).toBe(true);
  });

  it("blocks Start with an alert when no audio source is selected", async () => {
    renderApp("/record");
    await screen.findByText("Ready to record?");
    act(() => useRecordingStore.setState({ selected_sources: ["screen"] }));

    fireEvent.click(screen.getByRole("button", { name: "Start Recording" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/At least one audio source/);
    expect(useRecordingStore.getState().state).toBe("idle");
  });
});

describe("Integration: Meetings library", () => {
  it("searches, filters by classification and by date, and clears filters", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2027-01-01T00:00:00Z"));
    renderApp("/");
    expect(await screen.findByText(MEETING_TITLE)).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Search meetings"), { target: { value: "Consensus" } });
    expect(screen.getByText("Distributed Systems & Consensus Lecture")).toBeTruthy();
    expect(screen.queryByText(MEETING_TITLE)).toBeNull();
    fireEvent.change(screen.getByLabelText("Search meetings"), { target: { value: "" } });

    fireEvent.change(screen.getByLabelText("Filter meetings by classification"), { target: { value: "lecture" } });
    expect(screen.queryByText(MEETING_TITLE)).toBeNull();
    expect(screen.getByText("Distributed Systems & Consensus Lecture")).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Filter meetings by date range"), { target: { value: "today" } });
    expect(screen.getByText("No matching meetings found")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Clear Search & Filters" }));
    expect(await screen.findByText(MEETING_TITLE)).toBeTruthy();
  });

  it("opens a meeting from its card and keeps delete confirmation cancellable", async () => {
    renderApp("/");
    await screen.findByText(MEETING_TITLE);

    fireEvent.click(screen.getAllByTitle("Delete meeting")[0]);
    const dialog = screen.getByRole("dialog", { name: /delete meeting/i });
    fireEvent.click(within(dialog).getByRole("button", { name: /cancel/i }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByText(MEETING_TITLE)).toBeTruthy();

    fireEvent.click(screen.getByRole("article", { name: MEETING_TITLE }));
    expect(await h1(new RegExp(MEETING_TITLE))).toBeTruthy();
  });
});

describe("Integration: Meeting Detail", () => {
  it("switches tabs, opens the pipeline popover (same place, same a11y names) and the export dialog", async () => {
    renderApp("/meeting/m-01");
    expect(await h1(new RegExp(MEETING_TITLE))).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /^Transcript$/ }));
    expect(await screen.findByRole("feed", { name: "Synchronized meeting transcript" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /Action Items/ }));
    expect(await screen.findByText(tauri.MOCK_ACTIONS[0].text)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /^Chat$/ }));
    expect(await screen.findByRole("region", { name: "In-meeting scoped Q&A" })).toBeTruthy();

    const pill = await screen.findByRole("button", { name: /all pipeline steps complete/i });
    fireEvent.click(pill);
    const popover = screen.getByRole("dialog", { name: /granular pipeline status details/i });
    expect(within(popover).getByText("Diarization")).toBeTruthy();
    expect(within(popover).getByText("Summarization")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /Export/ }));
    expect(await screen.findByText(/export/i, { selector: "h2, h3" })).toBeTruthy();
  });

  it("goes back to the library", async () => {
    renderApp("/meeting/m-01");
    await h1(new RegExp(MEETING_TITLE));
    fireEvent.click(screen.getByRole("button", { name: "Back to meeting library" }));
    expect(await h1("Meetings")).toBeTruthy();
  });
});

describe("Integration: Model Manager (Hub-style cards + catalog fields)", () => {
  it("shows catalog fields only where the data has them", async () => {
    renderApp("/models");
    await h1("Model Manager");
    // Whisper tab: no catalog data, so no author / variants UI is invented.
    expect(await screen.findByText("Whisper Small (Bundled)")).toBeTruthy();
    expect(screen.queryByText(/^By /)).toBeNull();
    expect(screen.queryByText("Show variants")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Summarization (llama-server)" }));
    expect(await screen.findByText("By Meta")).toBeTruthy();
    expect(screen.getByText("By Mistral AI")).toBeTruthy();
    expect(screen.getByText("289,502")).toBeTruthy(); // download count, locale formatted
    expect(screen.getByText("✓ Fits")).toBeTruthy();
    expect(screen.getByText("May not fit")).toBeTruthy();
    expect(screen.getAllByText("Tools").length).toBe(2); // Llama + Mistral tag chips
    expect(screen.getByText("Long context")).toBeTruthy();
  });

  it("expands variants, each with a disabled Download pill (no download backend yet)", async () => {
    renderApp("/models");
    fireEvent.click(await screen.findByRole("button", { name: "Summarization (llama-server)" }));
    const toggles = await screen.findAllByRole("button", { name: /Show variants/ });
    expect(toggles[0].getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggles[0]);
    expect(toggles[0].getAttribute("aria-expanded")).toBe("true");

    const list = screen.getByRole("list", { name: "Llama 3.2 3B Instruct (GGUF) variants" });
    expect(within(list).getAllByRole("listitem").length).toBe(2);
    const download = within(list).getByRole("button", { name: "Download Q4_K_M" }) as HTMLButtonElement;
    expect(download.disabled).toBe(true);
  });

  it("Set Active selects the model and refreshes the list; the active model has no action button", async () => {
    const models = await actualTauri.listModels();
    const installed = { ...models.find((m) => m.id === "whisper-medium")!, status: "installed" as const };
    vi.mocked(tauri.listModels).mockResolvedValue([installed]);

    renderApp("/models");
    const setActive = await screen.findByRole("button", { name: "Set Active" });
    const callsBefore = vi.mocked(tauri.listModels).mock.calls.length;
    fireEvent.click(setActive);

    await waitFor(() => expect(tauri.selectModel).toHaveBeenCalledWith("whisper-medium", "whisper"));
    await waitFor(() => expect(vi.mocked(tauri.listModels).mock.calls.length).toBeGreaterThan(callsBefore));
  });

  it("shows live progress for a downloading model, on its card and in the sidebar popover", async () => {
    const models = await actualTauri.listModels();
    const downloading = {
      ...models.find((m) => m.id === "whisper-medium")!,
      status: "downloading" as const,
      download_progress: 30,
    };
    vi.mocked(tauri.listModels).mockResolvedValue([downloading]);

    renderApp("/models");
    const bar = await screen.findByRole("progressbar", { name: `Downloading ${downloading.name}` });
    expect(bar.getAttribute("aria-valuenow")).toBe("30");

    const aside = screen.getByLabelText("Primary navigation");
    fireEvent.click(within(aside).getByRole("button", { name: "Downloads" }));
    const popover = await screen.findByRole("dialog", { name: "Download progress" });
    expect(within(popover).getByText(downloading.name)).toBeTruthy();
  });
});

describe("Integration: Settings (two-pane, grouped cards)", () => {
  it("switches categories and marks the active one", async () => {
    renderApp("/settings");
    await h1("Settings");
    const nav = screen.getByRole("navigation", { name: "Settings categories" });
    expect(within(nav).getByRole("button", { name: "General" }).getAttribute("aria-current")).toBe("page");

    fireEvent.click(within(nav).getByRole("button", { name: "Privacy & Sovereignty" }));
    expect(within(nav).getByRole("button", { name: "Privacy & Sovereignty" }).getAttribute("aria-current")).toBe("page");
    expect(within(nav).getByRole("button", { name: "General" }).getAttribute("aria-current")).toBeNull();
    expect(screen.getByText("SHA-256 Enforced")).toBeTruthy();
    expect(screen.getByText("Disabled (0 bytes sent)")).toBeTruthy();
  });

  it("changes the theme from Appearance and persists it", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Appearance & Theme" }));

    const select = screen.getByLabelText("Theme") as HTMLSelectElement;
    expect(select.value).toBe("system");
    fireEvent.change(select, { target: { value: "dark" } });
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem("locus_theme_preference")).toBe("dark");
    expect(useUiStore.getState().theme).toBe("dark");

    fireEvent.change(select, { target: { value: "light" } });
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem("locus_theme_preference")).toBe("light");
  });

  it("flips capture-default switches", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Recording Defaults" }));
    const sw = screen.getByRole("switch", { name: "Default System Audio" });
    expect(sw.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(sw);
    expect(sw.getAttribute("aria-checked")).toBe("false");
  });

  it("shows both storage paths as copyable chips with a relocate action", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Storage & Relocation" }));
    expect((await screen.findAllByRole("button", { name: "Copy path" })).length).toBe(2);
    expect(screen.getByRole("button", { name: "Relocate Models…" })).toBeTruthy();
  });

  it("saves a provider key only once one is typed, and reports success", async () => {
    // Outside Tauri these commands throw "unavailable"; stub them as the desktop backend would answer.
    vi.mocked(tauri.configureProvider).mockResolvedValueOnce(undefined);
    vi.mocked(tauri.saveProviderApiKey).mockResolvedValueOnce(undefined);
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "OpenAI" }));

    const save = screen.getByRole("button", { name: "Save keychain credential" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Select" }));
    expect(screen.getAllByText("Selected (Active)").length).toBe(1);

    fireEvent.change(screen.getByLabelText("API Key (Redacted)"), { target: { value: "sk-test-123" } });
    expect(save.disabled).toBe(false);
    fireEvent.click(save);

    await waitFor(() => expect(tauri.configureProvider).toHaveBeenCalledWith("openai", "gpt-4o-mini", "https://api.openai.com", true));
    await waitFor(() => expect(tauri.saveProviderApiKey).toHaveBeenCalledWith("openai", "sk-test-123"));
    expect(await screen.findByText(/configured in the operating-system keychain/i)).toBeTruthy();
  });

  it("checks for updates and reports a status", async () => {
    renderApp("/settings");
    await h1("Settings");
    fireEvent.click(screen.getByRole("button", { name: "Check stable channel" }));
    const status = await screen.findByRole("status");
    expect(status.textContent).toMatch(/stable channel|up to date|available/i);
  });
});

describe("Integration: Knowledge Base (composer)", () => {
  it("enables Send only with text and shows the question in the thread", async () => {
    renderApp("/knowledge");
    await h1("Knowledge Base");

    const input = await screen.findByLabelText("Knowledge question");
    const send = screen.getByRole("button", { name: "Send" }) as HTMLButtonElement;
    expect(send.disabled).toBe(true);

    fireEvent.change(input, { target: { value: "What did we decide about the sidecar?" } });
    expect(send.disabled).toBe(false);
    fireEvent.click(send);

    await waitFor(() => expect(tauri.sendKnowledgeMessage).toHaveBeenCalled());
    expect(await screen.findByText("What did we decide about the sidecar?")).toBeTruthy();
    expect((screen.getByLabelText("Knowledge question") as HTMLInputElement).value).toBe("");
  });

  it("keeps document upload, semantic search and the index status reachable", async () => {
    renderApp("/knowledge");
    await h1("Knowledge Base");
    expect(await screen.findByLabelText("Upload document")).toBeTruthy();
    expect(screen.getByLabelText("Semantic search")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Run semantic search" })).toBeTruthy();
    expect(screen.getByText("Vector index")).toBeTruthy();
  });
});
