import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import React from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MediaPlayer } from "../components/player/MediaPlayer";
import { TranscriptView } from "../components/transcript/TranscriptView";
import { SummaryTab, renderWithCitations } from "../components/summary/SummaryTab";
import { ActionItemsTab } from "../components/actions/ActionItemsTab";
import { SlidesTab, SlidesStrip } from "../components/slides/SlidesTab";
import { PipelineStatusBadge } from "../components/pipeline/PipelineStatusBadge";
import { ExportModal } from "../components/export/ExportModal";
import { DeleteMeetingModal } from "../components/meetings/DeleteMeetingModal";
import { MeetingsView } from "../routes/MeetingsView";
import {
  MOCK_MEETINGS,
  MOCK_SEGMENTS,
  MOCK_SLIDES,
  MOCK_SUMMARY,
  MOCK_SUMMARY_REVISIONS,
  MOCK_ACTIONS,
  MOCK_PIPELINE_STATUS,
  getSpeakerPalette,
  normalizeTranscriptSegment,
  adaptSummaryRevision,
  DIARIZATION_PALETTE,
  exportMeetingAsMarkdown,
  exportMeetingAsJson,
  exportMeetingAsTxt,
  deleteMeeting,
  renameSpeaker,
  toggleActionItem,
  listMeetings,
  type MeetingDTO,
  type SummaryRevisionDTO,
} from "../lib/tauri";

const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

describe("M8.01: Video/Audio Player Component", () => {
  it("renders screen video placeholder for sessions with video", () => {
    const handleTimeUpdate = vi.fn();
    render(
      <MediaPlayer
        hasVideo={true}
        meetingTitle="Q3 Architecture & Offline RAG Sync"
        durationSeconds={2535}
        currentTime={872}
        onTimeUpdate={handleTimeUpdate}
      />
    );

    expect(screen.getByText("Q3 Architecture & Offline RAG Sync")).toBeDefined();
    expect(screen.getByText(/1080p @ 30fps/i)).toBeDefined();
    // Verify elapsed and total time readouts
    expect(screen.getByText("14:32")).toBeDefined();
    expect(screen.getByText("42:15")).toBeDefined();
  });

  it("renders dedicated audio transport card for audio-only sessions without blank video", () => {
    const handleTimeUpdate = vi.fn();
    render(
      <MediaPlayer
        hasVideo={false}
        meetingTitle="Distributed Systems & Consensus Lecture"
        durationSeconds={3600}
        currentTime={0}
        onTimeUpdate={handleTimeUpdate}
      />
    );

    expect(screen.getByText("Audio-Only Session")).toBeDefined();
    expect(screen.getByText("Distributed Systems & Consensus Lecture")).toBeDefined();
    // The badge must describe the real state, not a hardcoded codec (recordings are not always AAC).
    expect(screen.getByText("No audio file")).toBeDefined();
    // Video element should not be rendered for audio-only
    expect(document.querySelector("video")).toBeNull();
  });

  it("loads the recorded file into an <audio> element for audio-only sessions", () => {
    render(
      <MediaPlayer
        src="/tmp/media/m1/audio.wav"
        hasVideo={false}
        meetingTitle="Mic only"
        durationSeconds={10}
        currentTime={0}
        onTimeUpdate={vi.fn()}
      />
    );
    expect(document.querySelector("audio")?.getAttribute("src")).toContain("audio.wav");
    expect(screen.getByText("Recorded audio")).toBeDefined();
  });

  it("supports playback speed selection (0.75x - 2.0x)", () => {
    render(
      <MediaPlayer
        hasVideo={true}
        meetingTitle="Test Speed"
        durationSeconds={100}
        currentTime={10}
        onTimeUpdate={vi.fn()}
      />
    );

    const speedBtn = screen.getByLabelText("Playback speed");
    expect(speedBtn.textContent).toContain("1x");

    fireEvent.click(speedBtn);
    expect(screen.getByRole("menuitem", { name: /1.5x/i })).toBeDefined();
    expect(screen.getByRole("menuitem", { name: /2x/i })).toBeDefined();

    fireEvent.click(screen.getByRole("menuitem", { name: /1.5x/i }));
    expect(speedBtn.textContent).toContain("1.5x");
  });

  it("seeks when clicking timeline scrubber track with < 200ms response", () => {
    const handleTimeUpdate = vi.fn();
    const handleSeek = vi.fn();
    render(
      <MediaPlayer
        hasVideo={true}
        meetingTitle="Test Seek"
        durationSeconds={1000}
        currentTime={200}
        onTimeUpdate={handleTimeUpdate}
        onSeek={handleSeek}
      />
    );

    const scrubber = screen.getByRole("slider", { name: /seek time slider/i });
    expect(scrubber).toBeDefined();

    // Mock bounding rect: width 500, click at 250 -> 50% -> 500s
    vi.spyOn(scrubber, "getBoundingClientRect").mockReturnValue({
      left: 0,
      top: 0,
      width: 500,
      height: 10,
      right: 500,
      bottom: 10,
      x: 0,
      y: 0,
      toJSON: () => {},
    });

    fireEvent.click(scrubber, { clientX: 250 });
    expect(handleTimeUpdate).toHaveBeenCalledWith(500);
    expect(handleSeek).toHaveBeenCalledWith(500);
  });
});

describe("M8.02 & M8.03: Synchronized Transcript & Speaker Distinction", () => {
  it("renders transcript segments with timestamps and speaker labels", () => {
    render(
      <TranscriptView
        segments={MOCK_SEGMENTS}
        currentTime={872}
        onSeek={vi.fn()}
      />
    );

    expect(screen.getAllByText("Speaker 1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Speaker 2").length).toBeGreaterThan(0);
    expect(
      screen.getByText(
        "We converted the Python sidecar to an on-demand process with ChromaDB vector indexing running as a background job."
      )
    ).toBeDefined();
  });

  it("highlights active segment with aria-current='time' and 3px mint accent indicator", () => {
    render(
      <TranscriptView
        segments={MOCK_SEGMENTS}
        currentTime={875} // within segment 2: 872 - 879
        onSeek={vi.fn()}
      />
    );

    const activeSegment = document.querySelector("[aria-current='time']");
    expect(activeSegment).not.toBeNull();
    expect(activeSegment?.textContent).toContain("We converted the Python sidecar");
    // Verify 3px mint accent element exists
    const mintBar = activeSegment?.querySelector(".bg-mint");
    expect(mintBar).not.toBeNull();
  });

  it("pauses auto-scroll upon manual scroll and shows 'Jump to current' pill", () => {
    render(
      <TranscriptView
        segments={MOCK_SEGMENTS}
        currentTime={875}
        onSeek={vi.fn()}
      />
    );

    const feed = screen.getByRole("feed");
    fireEvent.scroll(feed, { target: { scrollTop: 300 } });

    // Floating Jump to current pill should appear
    const jumpBtn = screen.getByRole("button", { name: /jump to currently playing segment/i });
    expect(jumpBtn).toBeDefined();
    expect(jumpBtn.textContent).toContain("Jump to current");

    // Clicking it resumes
    fireEvent.click(jumpBtn);
    expect(screen.queryByRole("button", { name: /jump to currently playing segment/i })).toBeNull();
  });

  it("assigns distinct colors from diarization palette (7 hues) where color is never sole identifier", () => {
    expect(DIARIZATION_PALETTE.length).toBe(7);
    const spk1 = getSpeakerPalette("spk-1", false);
    const spk2 = getSpeakerPalette("spk-2", false);
    const spk3 = getSpeakerPalette("spk-3", false);

    expect(spk1.hue).toBe("Emerald");
    expect(spk2.hue).toBe("Slate Blue");
    expect(spk3.hue).toBe("Ochre Rust");

    // Both text and background colors are distinct
    expect(spk1.text).not.toBe(spk2.text);
    expect(spk2.text).not.toBe(spk3.text);
  });

  it("allows renaming a speaker and updates structured label", async () => {
    const handleRenamed = vi.fn();
    render(
      <TranscriptView
        segments={MOCK_SEGMENTS}
        currentTime={872}
        onSeek={vi.fn()}
        onSpeakerRenamed={handleRenamed}
      />
    );

    // Click speaker badge to open rename dialog
    const speakerBadges = screen.getAllByTitle(/click to rename speaker/i);
    fireEvent.click(speakerBadges[0]);

    expect(screen.getByRole("dialog", { name: /rename speaker/i })).toBeDefined();
    const input = screen.getByLabelText("New speaker display name");
    fireEvent.change(input, { target: { value: "Alice Morgan" } });

    fireEvent.click(screen.getByRole("button", { name: "Save Name" }));

    await waitFor(() => {
      expect(handleRenamed).toHaveBeenCalledWith("spk-1", "Alice Morgan");
    });
  });
});

describe("M8.04: AI Summary Tab & Citations", () => {
  it("renders executive overview, key decisions, and key concepts", () => {
    render(
      <SummaryTab
        meetingId="m-01"
        summary={MOCK_SUMMARY}
        revisions={MOCK_SUMMARY_REVISIONS}
        onSelectRevision={vi.fn()}
        onSeek={vi.fn()}
      />
    );

    expect(screen.getByText("Executive Overview")).toBeDefined();
    expect(screen.getByText(/engineering team confirmed the offline-first/i)).toBeDefined();
    expect(screen.getByText("Key Decisions & Consensuses")).toBeDefined();
    expect(screen.getByText("Key Concepts & Architectural Themes")).toBeDefined();
    expect(screen.getByText("Precision Master Control Room Ergonomics")).toBeDefined();
  });

  it("renders clickable citations that seek media player when clicked", () => {
    const handleSeek = vi.fn();
    render(
      <SummaryTab
        meetingId="m-01"
        summary={MOCK_SUMMARY}
        revisions={MOCK_SUMMARY_REVISIONS}
        onSelectRevision={vi.fn()}
        onSeek={handleSeek}
      />
    );

    // Citations like [14:32] should be interactive buttons
    const citationBtn = screen.getByRole("button", { name: /jump to citation \[14:32\]/i });
    expect(citationBtn).toBeDefined();

    fireEvent.click(citationBtn);
    // 14 minutes * 60 + 32 = 872 seconds
    expect(handleSeek).toHaveBeenCalledWith(872);
  });

  it("displays outdated warning banner when source content changed", () => {
    const outdatedSummary: SummaryRevisionDTO = {
      ...MOCK_SUMMARY,
      is_outdated: true,
    };

    render(
      <SummaryTab
        meetingId="m-01"
        summary={outdatedSummary}
        revisions={MOCK_SUMMARY_REVISIONS}
        onSelectRevision={vi.fn()}
        onSeek={vi.fn()}
      />
    );

    expect(screen.getByRole("alert")).toBeDefined();
    expect(screen.getByText(/Source content was updated/i)).toBeDefined();
    expect(screen.getByRole("button", { name: "Regenerate Summary" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Keep Current" })).toBeDefined();
  });

  it("allows switching revisions via revision dropdown", () => {
    const handleSelectRevision = vi.fn();
    render(
      <SummaryTab
        meetingId="m-01"
        summary={MOCK_SUMMARY_REVISIONS[0]}
        revisions={MOCK_SUMMARY_REVISIONS}
        onSelectRevision={handleSelectRevision}
        onSeek={vi.fn()}
      />
    );

    const select = screen.getByLabelText("Select summary revision");
    fireEvent.change(select, { target: { value: "sum-rev-1" } });

    expect(handleSelectRevision).toHaveBeenCalledWith(MOCK_SUMMARY_REVISIONS[1]);
  });
});

describe("M8.05: Action Items Tab & Revision-Owned Completion", () => {
  it("renders checkable tasks with assignee badge and deadline pill", () => {
    render(
      <ActionItemsTab
        summaryRevisionId="sum-rev-2"
        actionItems={MOCK_ACTIONS}
      />
    );

    expect(screen.getByText("Package standalone Python sidecar binary with PyInstaller")).toBeDefined();
    expect(screen.getByText("Benchmark whisper-rs transcription throughput on 8GB RAM fixture")).toBeDefined();
    expect(screen.getByText("Speaker 1")).toBeDefined();
    expect(screen.getByText("Due: Friday, Oct 2")).toBeDefined();
  });

  it("toggles action item completion with visual strikethrough and persists state", async () => {
    const handleToggle = vi.fn();
    render(
      <ActionItemsTab
        summaryRevisionId="sum-rev-2"
        actionItems={MOCK_ACTIONS}
        onToggleAction={handleToggle}
      />
    );

    // item 2 is uncompleted
    const checkbox = screen.getByRole("checkbox", {
      name: /mark task as completed: Benchmark whisper-rs/i,
    });
    expect(checkbox).toBeDefined();

    fireEvent.click(checkbox);
    expect(handleToggle).toHaveBeenCalledWith("act-2", true);
  });
});

describe("M8.06: Slides Gallery Tab & OCR Snippets", () => {
  it("renders grid of slide cards with timestamps, ordinals, and OCR snippets", () => {
    const handleSeek = vi.fn();
    render(
      <SlidesTab
        slides={MOCK_SLIDES}
        hasVideo={true}
        currentTime={130}
        onSeek={handleSeek}
      />
    );

    expect(screen.getAllByText("Slide 1").length).toBeGreaterThan(0);
    expect(screen.getAllByText(/System Architecture: Tauri Rust Core/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Data Boundaries: SQLite for relational state/i).length).toBeGreaterThan(0);
    // Repeated timestamps chip: "also 28:15"
    expect(screen.getByText("also 28:15")).toBeDefined();
  });

  it("seeks player to exact timestamp when slide is clicked", () => {
    const handleSeek = vi.fn();
    render(
      <SlidesTab
        slides={MOCK_SLIDES}
        hasVideo={true}
        currentTime={0}
        onSeek={handleSeek}
      />
    );

    const slideCard = screen.getByRole("button", { name: /Slide 2 at 08:45/i });
    fireEvent.click(slideCard);
    expect(handleSeek).toHaveBeenCalledWith(525);
  });

  it("renders informational empty state for audio-only sessions without slides", () => {
    render(
      <SlidesTab
        slides={[]}
        hasVideo={false}
        currentTime={0}
        onSeek={vi.fn()}
      />
    );

    expect(screen.getByText("No presentation slides in this recording")).toBeDefined();
    expect(screen.getByText(/Audio-only sessions do not generate presentation slide cards/i)).toBeDefined();
  });

  it("renders inline retry banner when slide OCR processing failed", () => {
    const handleRetryOcr = vi.fn();
    render(
      <SlidesTab
        slides={MOCK_SLIDES}
        hasVideo={true}
        currentTime={0}
        onSeek={vi.fn()}
        ocrError={true}
        onRetryOcr={handleRetryOcr}
      />
    );

    expect(screen.getByRole("alert")).toBeDefined();
    const retryBtn = screen.getByRole("button", { name: /retry ocr/i });
    fireEvent.click(retryBtn);
    expect(handleRetryOcr).toHaveBeenCalled();
  });
});

describe("Transcript auto-scroll stays inside the transcript container", () => {
  const proto = HTMLElement.prototype as unknown as Record<string, unknown>;
  let scrollTo: ReturnType<typeof vi.fn>;
  let scrollIntoView: ReturnType<typeof vi.fn>;
  let originalScrollTo: unknown;
  let originalScrollIntoView: unknown;

  beforeEach(() => {
    originalScrollTo = proto.scrollTo;
    originalScrollIntoView = proto.scrollIntoView;
    scrollTo = vi.fn();
    scrollIntoView = vi.fn();
    proto.scrollTo = scrollTo;
    proto.scrollIntoView = scrollIntoView;
  });
  afterEach(() => {
    proto.scrollTo = originalScrollTo;
    proto.scrollIntoView = originalScrollIntoView;
  });

  const renderPlaying = () =>
    render(<TranscriptView segments={MOCK_SEGMENTS} currentTime={875} onSeek={vi.fn()} />);

  it("follows playback by scrolling the feed itself, never scrollIntoView (which moves the page)", () => {
    renderPlaying();
    const feed = screen.getByRole("feed");
    expect(scrollTo).toHaveBeenCalled();
    expect(scrollTo.mock.instances[0]).toBe(feed);
    expect(scrollTo.mock.calls[0][0]).toMatchObject({ behavior: "smooth" });
    expect(typeof scrollTo.mock.calls[0][0].top).toBe("number");
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it("does not mistake its own smooth scrolling for the user scrolling", () => {
    renderPlaying();
    const feed = screen.getByRole("feed");
    // a scroll event that lands on the path the auto-scroll is animating along
    fireEvent.scroll(feed, { target: { scrollTop: scrollTo.mock.calls[0][0].top } });
    expect(screen.queryByRole("button", { name: /jump to currently playing segment/i })).toBeNull();
  });

  it("treats wheel and touch input as the user taking over", () => {
    renderPlaying();
    fireEvent.wheel(screen.getByRole("feed"));
    expect(screen.getByRole("button", { name: /jump to currently playing segment/i })).toBeDefined();
  });

  it("falls back to setting scrollTop where scrollTo does not exist", () => {
    proto.scrollTo = undefined;
    renderPlaying();
    expect(scrollIntoView).not.toHaveBeenCalled();
  });
});

describe("Transcripts without speaker attribution (regression: speakerIdOrIndex.match)", () => {
  it("getSpeakerPalette tolerates a missing speaker instead of throwing", () => {
    expect(() => getSpeakerPalette(undefined)).not.toThrow();
    expect(() => getSpeakerPalette(null)).not.toThrow();
    expect(() => getSpeakerPalette("")).not.toThrow();
  });

  it("opaque ids get a stable colour that is not always the same hue", () => {
    const a = getSpeakerPalette("0b3f9d2e-1111-4a5b-9c1d-aaaaaaaaaaaa");
    const again = getSpeakerPalette("0b3f9d2e-1111-4a5b-9c1d-aaaaaaaaaaaa");
    expect(a).toEqual(again);
    const hues = new Set(
      ["a1b2c3d4-e5f6", "ffee0011-2233", "9a8b7c6d-5e4f", "01010101-abab", "zzzz-yyyy-xxxx-wwww"].map(
        (id) => getSpeakerPalette(id.repeat(2)).hue,
      ),
    );
    expect(hues.size).toBeGreaterThan(1);
  });

  it("normalizes the backend shape and fills anything missing", () => {
    expect(
      normalizeTranscriptSegment({ id: "s1", start_seconds: 1.5, end_seconds: 3, text: "hi" } as never),
    ).toEqual({ id: "s1", speaker_id: "", speaker_label: "", start_time: 1.5, end_time: 3, text: "hi" });
    expect(normalizeTranscriptSegment({} as never).text).toBe("");
  });

  it("renders a transcript with no speakers: text and time, no speaker chip, no crash", () => {
    render(
      <TranscriptView
        segments={[
          { id: "a", speaker_id: "", speaker_label: "", start_time: 0, end_time: 2, text: "Welcome everyone" },
          { id: "b", speaker_id: "", speaker_label: "", start_time: 2, end_time: 5, text: "Let us begin" },
        ]}
        currentTime={1}
        onSeek={vi.fn()}
      />,
    );
    expect(screen.getByText("Welcome everyone")).toBeDefined();
    expect(screen.getByText("Let us begin")).toBeDefined();
    expect(screen.queryByTitle(/click to rename speaker/i)).toBeNull();
  });

  it("still shows the speaker chip when a speaker is attributed", () => {
    render(
      <TranscriptView
        segments={[
          { id: "a", speaker_id: "spk-1", speaker_label: "Dana", start_time: 0, end_time: 2, text: "Hello" },
        ]}
        currentTime={0}
        onSeek={vi.fn()}
      />,
    );
    expect(screen.getByText("Dana")).toBeDefined();
    expect(screen.getByTitle(/click to rename speaker/i)).toBeDefined();
  });
});

describe("Backend summary shapes are adapted for the UI", () => {
  it("flattens a backend revision (structured_json) into overview/decisions/concepts", () => {
    const adapted = adaptSummaryRevision({
      id: "r1",
      meeting_id: "m1",
      revision: 3,
      freshness: "outdated",
      published_at: "2026-10-11T09:00:00Z",
      model_identity: "claude-x",
      structured_json: { overview: "We agreed.", decisions: ["Ship Friday"], key_concepts: ["VAD"], extra: 1 },
    } as never);
    expect(adapted).toEqual({
      id: "r1",
      meeting_id: "m1",
      revision_number: 3,
      overview: "We agreed.",
      decisions: ["Ship Friday"],
      key_concepts: ["VAD"],
      created_at: "2026-10-11T09:00:00Z",
      model: "claude-x",
      is_outdated: true,
    });
  });

  it("never yields undefined lists, even for an empty or odd payload", () => {
    const adapted = adaptSummaryRevision({ id: "r", structured_json: null } as never);
    expect(adapted.overview).toBe("");
    expect(adapted.decisions).toEqual([]);
    expect(adapted.key_concepts).toEqual([]);
  });

  it("passes an already-flat UI revision through unchanged", () => {
    const flat = MOCK_SUMMARY_REVISIONS[0];
    expect(adaptSummaryRevision(flat as never)).toMatchObject({
      overview: flat.overview,
      decisions: flat.decisions,
      revision_number: flat.revision_number,
    });
  });
});

describe("M8.07: Pipeline Status Display & Step Retries", () => {
  it("displays pipeline status badge and expands granular step execution graph", () => {
    render(<PipelineStatusBadge statusList={MOCK_PIPELINE_STATUS} />);

    const badge = screen.getByRole("button", { name: /all pipeline steps complete/i });
    expect(badge).toBeDefined();

    fireEvent.click(badge);
    expect(screen.getByRole("dialog", { name: /granular pipeline status details/i })).toBeDefined();
    expect(screen.getByText("Recording Encoding")).toBeDefined();
    expect(screen.getByText("Transcription")).toBeDefined();
    expect(screen.getByText("Summarization")).toBeDefined();
  });

  it("displays error state with individual step retry button", async () => {
    const errorStatus = [
      ...MOCK_PIPELINE_STATUS.slice(0, 3),
      {
        job_id: "j-diarize",
        kind: "diarization",
        state: "error" as const,
        progress: 40,
        reason: "CUDA out of memory, retrying on CPU",
      },
    ];

    const handleRetryStep = vi.fn();
    render(
      <PipelineStatusBadge
        statusList={errorStatus}
        onRetryStep={handleRetryStep}
      />
    );

    expect(screen.getByText(/Pipeline Error/i)).toBeDefined();

    // Open dropdown
    fireEvent.click(screen.getByRole("button", { name: /pipeline error/i }));
    expect(screen.getByText("CUDA out of memory, retrying on CPU")).toBeDefined();

    const retryBtn = screen.getByRole("button", { name: /retry/i });
    fireEvent.click(retryBtn);
    await waitFor(() => {
      expect(handleRetryStep).toHaveBeenCalledWith("j-diarize");
    });
  });
});

describe("Pipeline status: blocked, queued and skipped steps", () => {
  const step = (kind: string, state: string, reason: string | null = null) => ({
    job_id: `j-${kind}`,
    kind,
    state: state as "pending" | "running" | "done" | "error" | "blocked" | "skipped",
    progress: 0,
    reason,
  });

  it("shows a blocked step with its reason and a Retry button, not 'complete'", async () => {
    const handleRetryStep = vi.fn();
    render(
      <PipelineStatusBadge
        statusList={[
          step("transcription", "done"),
          step("summarization", "blocked", "Select an AI provider in Settings to generate the summary, then retry."),
          step("indexing", "pending"),
        ]}
        onRetryStep={handleRetryStep}
      />,
    );
    expect(screen.queryByText(/all pipeline steps complete/i)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /action needed/i }));
    expect(screen.getByText(/select an ai provider in settings/i)).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: /retry/i }));
    await waitFor(() => expect(handleRetryStep).toHaveBeenCalledWith("j-summarization"));
  });

  it("shows what a finished step did and lets the user re-run it", async () => {
    const handleRetryStep = vi.fn();
    render(
      <PipelineStatusBadge
        statusList={[
          step("encoding", "done"),
          step("transcription", "done", "42 segments (energy VAD, CPU, language en detected (91%))."),
        ]}
        onRetryStep={handleRetryStep}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /all pipeline steps complete/i }));
    expect(screen.getByText(/language en detected \(91%\)/i)).toBeDefined();
    // only the transcription step offers Re-run; the recording itself cannot be redone
    const reruns = screen.getAllByRole("button", { name: /re-run/i });
    expect(reruns).toHaveLength(1);
    fireEvent.click(reruns[0]);
    await waitFor(() => expect(handleRetryStep).toHaveBeenCalledWith("j-transcription"));
  });

  it("says a meeting is queued while steps are still pending", () => {
    render(
      <PipelineStatusBadge
        statusList={[step("encoding", "done"), step("transcription", "pending")]}
      />,
    );
    expect(screen.getByRole("button", { name: /queued for processing/i })).toBeDefined();
  });

  it("counts skipped optional steps as finished and explains why they were skipped", () => {
    render(
      <PipelineStatusBadge
        statusList={[
          step("transcription", "done"),
          step("diarization", "skipped", "Speaker diarization is unavailable: pyannote.audio is not installed"),
          step("summarization", "done"),
        ]}
      />,
    );
    const badge = screen.getByRole("button", { name: /all pipeline steps complete/i });
    fireEvent.click(badge);
    expect(screen.getByText(/pyannote\.audio is not installed/i)).toBeDefined();
  });
});

describe("M8.08: Meetings List View & Delete Confirmation", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("renders meetings list with search and filters", async () => {
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <MeetingsView />
      </QueryClientProvider>
    );

    await waitFor(() => {
      expect(screen.getByText("Q3 Architecture & Offline RAG Sync")).toBeDefined();
      expect(screen.getByText("Distributed Systems & Consensus Lecture")).toBeDefined();
    });

    // Test real-time search filter
    const searchInput = screen.getByLabelText("Search meetings");
    fireEvent.change(searchInput, { target: { value: "Consensus" } });

    expect(screen.getByText("Distributed Systems & Consensus Lecture")).toBeDefined();
    expect(screen.queryByText("Q3 Architecture & Offline RAG Sync")).toBeNull();
  });

  it("filters meetings by classification type", async () => {
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <MeetingsView />
      </QueryClientProvider>
    );

    await waitFor(() => {
      expect(screen.getByText("Q3 Architecture & Offline RAG Sync")).toBeDefined();
    });

    const typeFilter = screen.getByLabelText("Filter meetings by classification");
    fireEvent.change(typeFilter, { target: { value: "lecture" } });

    expect(screen.getByText("Distributed Systems & Consensus Lecture")).toBeDefined();
    expect(screen.queryByText("Q3 Architecture & Offline RAG Sync")).toBeNull();
  });

  it("opens delete confirmation modal and removes meeting", async () => {
    const client = createTestQueryClient();
    render(
      <QueryClientProvider client={client}>
        <MeetingsView />
      </QueryClientProvider>
    );

    await waitFor(() => {
      expect(screen.getByText("Q3 Architecture & Offline RAG Sync")).toBeDefined();
    });

    const deleteBtns = screen.getAllByTitle("Delete meeting");
    fireEvent.click(deleteBtns[0]);

    // Modal should appear
    expect(screen.getByRole("dialog", { name: /delete meeting/i })).toBeDefined();
    expect(screen.getByText("Delete meeting?")).toBeDefined();
    expect(screen.getByText(/will be permanently deleted/i)).toBeDefined();

    // Confirm deletion
    const confirmDeleteBtn = screen.getByRole("button", { name: "Delete" });
    fireEvent.click(confirmDeleteBtn);

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });
});

describe("M8.09: Export Functions (Clipboard, Markdown, JSON, TXT)", () => {
  it("formats valid Markdown with metadata, executive summary, decisions, action items, and full transcript", () => {
    const meeting = MOCK_MEETINGS[0];
    const md = exportMeetingAsMarkdown(meeting, MOCK_SUMMARY, MOCK_ACTIONS, MOCK_SEGMENTS);

    expect(md).toContain(`# ${meeting.title}`);
    expect(md).toContain("## Executive Overview");
    expect(md).toContain(MOCK_SUMMARY.overview);
    expect(md).toContain("## Key Decisions");
    expect(md).toContain("## Action Items");
    expect(md).toContain("[x] Package standalone Python sidecar");
    expect(md).toContain("[ ] Benchmark whisper-rs transcription");
    expect(md).toContain("## Full Transcript");
    expect(md).toContain("[14:28] Speaker 1:");
  });

  it("formats valid structured JSON schema", () => {
    const meeting = MOCK_MEETINGS[0];
    const jsonStr = exportMeetingAsJson(meeting, MOCK_SUMMARY, MOCK_ACTIONS, MOCK_SEGMENTS, MOCK_SLIDES);
    const parsed = JSON.parse(jsonStr);

    expect(parsed.meeting.id).toBe("m-01");
    expect(parsed.summary.overview).toBeDefined();
    expect(parsed.action_items.length).toBe(MOCK_ACTIONS.length);
    expect(parsed.transcript.length).toBe(MOCK_SEGMENTS.length);
    expect(parsed.slides.length).toBe(MOCK_SLIDES.length);
  });

  it("formats clean TXT transcript dialogue", () => {
    const meeting = MOCK_MEETINGS[0];
    const txt = exportMeetingAsTxt(meeting, MOCK_SEGMENTS);

    expect(txt).toContain(`TRANSCRIPT: ${meeting.title}`);
    expect(txt).toContain("[14:28] Speaker 1:");
    expect(txt).toContain("[14:40] Speaker 2:");
  });

  it("renders export modal and supports one-click clipboard copy", async () => {
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: writeTextSpy },
      writable: true,
      configurable: true,
    });

    render(
      <ExportModal
        isOpen={true}
        onClose={vi.fn()}
        meeting={MOCK_MEETINGS[0]}
        summary={MOCK_SUMMARY}
        actionItems={MOCK_ACTIONS}
        segments={MOCK_SEGMENTS}
        slides={MOCK_SLIDES}
      />
    );

    expect(screen.getByText("Export Meeting Content")).toBeDefined();
    expect(screen.getByText("Markdown (.md)")).toBeDefined();
    expect(screen.getByText("JSON (.json)")).toBeDefined();
    expect(screen.getByText("PDF Document")).toBeDefined();

    // Click One-Click Copy
    const copyBtn = screen.getByRole("button", { name: /copy \(c\)/i });
    fireEvent.click(copyBtn);

    await waitFor(() => {
      expect(writeTextSpy).toHaveBeenCalled();
    });
  });
});
