# Locus — Frontend Design & Visual System

> **Document Classification**: Frontend Visual Architecture & Design System  
> **Source of Truth**: This document defines the visual world, design tokens, component architecture, layout topology, and interaction specifications for the Locus frontend. For backend systems, Rust orchestration, Tauri IPC, and sidecar architecture, see [docs/plans/DESIGN.md](docs/plans/DESIGN.md). For observable behavior and acceptance criteria, see [docs/plans/SPEC.md](docs/plans/SPEC.md). For product requirements, see [docs/plans/PRD.md](docs/plans/PRD.md).

---

## 1. Design Direction Contract & Creative Philosophy

<!-- impeccable:direction-contract
THESIS: Precision Audio-Visual Master Control Room combined with Archival Knowledge Curation. Rejects generic vibecoded neon purples, floating glassmorphism cards, and low-contrast green body text. Every stream (video, audio tracks, transcript segments, slide deck, AI synthesized intelligence) is treated as a calibrated, synchronized track on an ergonomic desktop console.
OWN-WORLD: Editorial Botanical palette—deep pine charcoal ink (#19211D) set against warm architectural paper-beige (#FAF8F5) in Light Mode, obsidian slate (#111614) in Dark Mode, with signature evergreen (#419873), forest emerald (#49ab81), and luminous mint (#52bf90) carrying active transport state, recording meters, active transcript line tracking, and verified citation links.
STORY: The user feels total confidence and sovereign ownership over their meetings: recording starts in ≤2 clicks, long sessions run without distraction, and post-session review presents synchronized media, searchable dialogue, extracted presentation slides, and checkable action items in an uncluttered, high-craft studio workspace.
FIRST VIEWPORT: Persistent left sidebar (240px) with clean navigation and offline/local status pill; main Meeting Detail studio featuring a split layout with synchronized 16:9 video player and horizontal slide strip on the left, and timestamped transcript with active segment highlight bar and tabbed AI summaries on the right.
FORM: Candidate 6 (The Precision Master Control Room) from grounded studio systems; seed key 5a80d19a.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance.
-->

### 1.1 The Anti-Vibecoding Philosophy

Modern AI-assisted interfaces have saturated into a predictable set of visual tropes: blurry glassmorphism cards with multi-colored pastel shadows, floating ungrounded elements, generic neon purple or electric violet buttons, and soft, unreadable pastel text.

**Locus explicitly rejects these tropes.**

Instead, Locus draws its visual character from:
1. **Precision Audio Engineering Consoles** (Nagra, Studer, Revox, and Dieter Rams / Braun audio gear): Tactile, functional, high-density, reliable, and physical. Controls communicate state instantly through clean mechanical affordances, clear debossed channels, and crisp LED-calibrated level indicators.
2. **Archival Research Library Workstations**: High typographic contrast, generous but disciplined leading, clear structural hierarchy, footnote citations that navigate directly to primary sources, and a warm, calming paper ground that permits hours of intensive reading without retinal fatigue.
3. **Swiss Typographic Clarity** (International Typographic Style): Grid-aligned layouts, tabular alignment of numbers and timestamps, strict visual weighting, and zero gratuitous ornamentation.

---

## 2. Color System & Ergonomic Counter-Proposal

### 2.1 Analysis & Critique of User Proposed Palette

The user initially proposed:
- Green hexes: `#419873`, `#49ab81`, `#52bf90` for text and other elements.
- Light mode ground: Warm beige.

#### The Contrast & Readability Breakdown
- **Luminance of `#419873`**: ~0.247. On a warm paper ground (`#FAF8F5`, L ~0.95), the contrast ratio is only **3.3:1**.
- **Luminance of `#49ab81`**: ~0.320. On `#FAF8F5`, contrast drops to **2.6:1**.
- **Luminance of `#52bf90`**: ~0.420. On `#FAF8F5`, contrast drops to **2.1:1**.

> [!CAUTION]
> **WCAG 2.1 AA Violation**: Normal body text requires a minimum contrast ratio of **4.5:1** (Level AA) and recommends **7:1** (Level AAA). Using `#419873`, `#49ab81`, or `#52bf90` for body text, transcripts, or summaries fails accessibility standards and produces immediate eye strain during extended reading.

#### The Architectural Solution: Editorial Botanical
We preserve the user's aesthetic vision while establishing rigorous desktop ergonomics:
1. **Body Typography & Deep Ink**: General body copy, transcript sentences, and summary prose use **Deep Botanical Charcoal-Pine** (`#19211D` / `oklch(0.185 0.012 160)`). On our architectural paper-beige ground (`#FAF8F5`), this delivers a contrast ratio of **13.8:1** (exceeding WCAG AAA).
2. **Signature Greens as Functional Accents**: The proposed greens are re-assigned to high-value brand and interactive roles where their chromatic energy excels:
   - `#419873` (`oklch(0.612 0.118 158.0)`): **Primary Brand & Active State** — Active sidebar pill, recording transport button, media scrubber progress bar, and primary action fills (carrying pure white text).
   - `#49ab81` (`oklch(0.672 0.132 158.0)`): **Interactive Emerald** — Hover states, focus rings, and secondary interactive toggles.
   - `#52bf90` (`oklch(0.735 0.144 158.0)`): **Luminous Mint** — Active transcript segment indicator bar, live audio level meter peaks, and luminous dark mode accents.
   - `#265C44` (`oklch(0.420 0.095 158.0)`): **Accessible Green Text & Badges** — Specially calibrated deep emerald with a contrast ratio of **6.2:1** on beige, ensuring green metadata tags and badges are 100% WCAG AA compliant.

---

### 2.2 Complete OKLCH Design Tokens

All tokens are defined in **OKLCH** per the Impeccable standard, guaranteeing perceptual uniformity across displays.

#### Light Mode (Architectural Paper Studio)

```css
:root, [data-theme="light"] {
  /* Surfaces & Grounds */
  --color-bg: oklch(0.978 0.006 85.0);           /* #FAF8F5 - Warm architectural paper ground */
  --color-surface: oklch(0.952 0.008 85.0);      /* #F3EFEA - Linen card & pane container */
  --color-surface-elevated: oklch(1.0 0 0);      /* #FFFFFF - Pure white elevated panels, dropdowns, modals */
  --color-surface-sunken: oklch(0.935 0.010 85.0); /* #EBE6DF - Sunken gutters, scroll tracks */

  /* Borders & Hairlines */
  --color-border: oklch(0.902 0.010 85.0);       /* #E5E0D6 - Subtle 1px structural dividing lines */
  --color-border-subtle: oklch(0.925 0.008 85.0);/* #ECE7DE - Muted internal cell dividers */
  --color-border-active: oklch(0.612 0.118 158.0);/* #419873 - Focused & selected borders */

  /* Typography & Ink */
  --color-ink: oklch(0.185 0.012 160.0);         /* #19211D - Deep botanical pine-charcoal (13.8:1 contrast) */
  --color-ink-muted: oklch(0.485 0.012 160.0);   /* #5D6862 - Secondary copy, timestamps, captions (4.8:1) */
  --color-ink-subtle: oklch(0.650 0.010 160.0);  /* #89938D - De-emphasized placeholders, inactive icons */

  /* Signature Brand & Accent Greens */
  --color-primary: oklch(0.612 0.118 158.0);     /* #419873 - Primary evergreen action fill */
  --color-primary-hover: oklch(0.570 0.125 158.0);/* #388463 - Primary button hover */
  --color-accent: oklch(0.672 0.132 158.0);      /* #49AB81 - Interactive hover & secondary controls */
  --color-mint: oklch(0.735 0.144 158.0);        /* #52BF90 - Luminous transcript bar & level peak */
  --color-green-text: oklch(0.420 0.095 158.0);  /* #265C44 - High-contrast green text/badges (6.2:1) */
  --color-green-tint: oklch(0.945 0.035 158.0);  /* #E2F3EB - Soft badge background / line highlight fill */

  /* Functional Status Roles */
  --color-status-recording: oklch(0.62 0.22 25.0); /* #E53935 - Recording pulse beacon (crimson) */
  --color-status-success: oklch(0.58 0.14 150.0);  /* #2E7D32 - Done & completed */
  --color-status-warning: oklch(0.72 0.16 75.0);   /* #D97706 - Outdated result / warning */
  --color-status-error: oklch(0.55 0.20 28.0);     /* #C53030 - Error state / pipeline failure */
  --color-status-pending: oklch(0.68 0.02 85.0);   /* #9CA3AF - Idle / queued / skipped */
}
```

#### Dark Mode (Obsidian Studio Slate)

```css
[data-theme="dark"] {
  /* Surfaces & Grounds */
  --color-bg: oklch(0.150 0.010 160.0);           /* #111614 - Deep obsidian slate ground */
  --color-surface: oklch(0.200 0.012 160.0);      /* #18201C - Studio panel & card surface */
  --color-surface-elevated: oklch(0.245 0.015 160.0);/* #202A25 - Elevated modals, popovers, dropdowns */
  --color-surface-sunken: oklch(0.125 0.008 160.0); /* #0C100F - Sunken gutters & input fields */

  /* Borders & Hairlines */
  --color-border: oklch(0.290 0.015 160.0);       /* #28352F - Subtle dark dividing hairlines */
  --color-border-subtle: oklch(0.230 0.012 160.0);/* #1E2723 - Internal cell dividers */
  --color-border-active: oklch(0.735 0.144 158.0);/* #52BF90 - Luminous mint active border */

  /* Typography & Ink */
  --color-ink: oklch(0.960 0.005 160.0);         /* #F0F3F1 - Crisp off-white body copy (14.5:1 contrast) */
  --color-ink-muted: oklch(0.670 0.015 160.0);   /* #8E9C95 - Secondary text & timestamps (6.1:1) */
  --color-ink-subtle: oklch(0.480 0.012 160.0);  /* #57635D - Muted icons & placeholders */

  /* Signature Brand & Accent Greens */
  --color-primary: oklch(0.735 0.144 158.0);     /* #52BF90 - Luminous mint action fill */
  --color-primary-hover: oklch(0.780 0.150 158.0);/* #66D4A4 - Hover state */
  --color-accent: oklch(0.672 0.132 158.0);      /* #49AB81 - Secondary accent */
  --color-mint: oklch(0.735 0.144 158.0);        /* #52BF90 - High-contrast indicators */
  --color-green-text: oklch(0.780 0.130 158.0);  /* #64D6A2 - Accessible green text in dark mode */
  --color-green-tint: oklch(0.250 0.050 158.0);  /* #173629 - Dark green pill fill */

  /* Functional Status Roles */
  --color-status-recording: oklch(0.68 0.22 25.0); /* #EF4444 */
  --color-status-success: oklch(0.68 0.16 150.0);  /* #34D399 */
  --color-status-warning: oklch(0.78 0.16 75.0);   /* #FBBF24 */
  --color-status-error: oklch(0.65 0.20 28.0);     /* #F87171 */
  --color-status-pending: oklch(0.50 0.02 160.0);  /* #6B7280 */
}
```

---

### 2.3 Speaker Diarization Palette

In meetings with multiple speakers, pyannote diarizes both audio streams by default, preserving source provenance. Distinct speakers receive a consistent hue paired with an accessible dark ink and subtle pill background. Device or microphone input never automatically implies user identity; only an explicit “Only me on this microphone” recording setting assigns microphone speech to the User. Otherwise, speakers are labeled anonymously (`Speaker 1`, `Speaker 2`, etc.) until user rename:

| Speaker Slot | Label Color (Light) | Pill Tint (Light) | Label Color (Dark) | Pill Tint (Dark) | Identity / Provenance Default |
|---|---|---|---|---|---|
| **Speaker 1** | `#265C44` (Emerald) | `#E2F3EB` | `#64D6A2` | `#173629` | Anonymous voice / User only if “Only me on mic” enabled |
| **Speaker 2** | `#2B5B84` (Slate Blue) | `#E3EDF5` | `#78B3E4` | `#1A2F42` | Anonymous detected participant |
| **Speaker 3** | `#7A4B27` (Ochre Rust) | `#F5ECE3` | `#E09A64` | `#3D2717` | Anonymous detected participant |
| **Speaker 4** | `#6B3D6B` (Plum) | `#F3E8F3` | `#C688C6` | `#351D35` | Anonymous detected participant |
| **Speaker 5** | `#2C6661` (Teal) | `#E2F0EF` | `#6CC7BF` | `#163330` | Anonymous detected participant |
| **Speaker 6** | `#73561E` (Warm Olive) | `#F4EFE0` | `#D6AE56` | `#3A2B0F` | Anonymous detected participant |
| **Speaker 7+** | `#525866` (Neutral Steel) | `#EAECEF` | `#A3ABB8` | `#252830` | Anonymous detected participant |

> [!IMPORTANT]
> **Accessibility & Identity Rules**:
> 1. Color is never the sole indicator of speaker identity. Speaker labels (e.g. `Speaker 1` or renamed `Morgan`) and start/end timestamps are always explicitly rendered in text beside each segment.
> 2. Device input alone does not establish personal identity. Renaming a speaker updates structured references across the meeting without rewriting raw transcript text or quoted evidence. Unknown speakers remain labeled as unknown.

---

## 3. Typography & Spacing Scale

### 3.1 Font Families & System Discipline

1. **Workhorse Desktop UI Face**:
   - `font-family: -apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", Roboto, "Helvetica Neue", sans-serif;`
   - High legibility, crisp hinting at small sizes (11px–13px), native OS feel on macOS, Windows, and Linux.
2. **Tabular Monospace Face**:
   - `font-family: "JetBrains Mono", "SF Mono", Menlo, Consolas, monospace;`
   - Mandatory for all timestamps (`00:14:32 / 00:42:15`), audio levels (`-18 dBFS`), pipeline progress percentages (`45%`), and step runtimes (`1.4s`).
   - Paired with `font-variant-numeric: tabular-nums;` to eliminate layout jitter during live counter updates.

### 3.2 Typographic Hierarchy

| Token | Size | Line Height | Weight | Letter Spacing | Purpose |
|---|---|---|---|---|---|
| `text-display` | 24px (1.5rem) | 32px | 600 (SemiBold) | -0.02em | Main Meeting Title, View Headlines |
| `text-title` | 18px (1.125rem) | 26px | 600 (SemiBold) | -0.015em | Panel headers, Section anchors |
| `text-headline` | 15px (0.9375rem) | 22px | 600 (SemiBold) | -0.01em | Card titles, Modal headers, Tab labels |
| `text-body` | 14px (0.875rem) | 22px | 400 (Regular) | 0 | Primary reading copy, transcripts, summaries |
| `text-body-bold` | 14px (0.875rem) | 22px | 600 (SemiBold) | 0 | Action item assignees, highlighted phrases |
| `text-caption` | 12px (0.75rem) | 16px | 500 (Medium) | +0.01em | Metadata badges, speaker tags, timestamps |
| `text-mono` | 12px (0.75rem) | 16px | 400 (Regular) | 0 | Tabular scrub timestamps, audio level readouts |
| `text-micro` | 11px (0.6875rem) | 14px | 500 (Medium) | +0.02em | Keyboard shortcut badges, step status labels |

### 3.3 Spacing & Rhythm Grid

An 8-point base grid is strictly followed, with a 4-point micro-grid for compact controls:

- `space-1`: 4px (tight badge padding, inline icon spacing)
- `space-2`: 8px (button internal padding, gutter spacing)
- `space-3`: 12px (sidebar item padding, card inner margins)
- `space-4`: 16px (standard container padding, split view gutter)
- `space-6`: 24px (section headers, modal inner padding)
- `space-8`: 32px (major viewport dividers)

---

## 4. Application Surface Architecture & Views

### 4.1 Master Frame & Sidebar Navigation

The top-level desktop layout follows a persistent sidebar pattern:

```
+----------------------------------------------------------------------------------+
| macOS Window Controls (Traffic Lights) / Windows Title Bar                       |
+-------------------+--------------------------------------------------------------+
| [LOCUS LOGO]      | VIEW HEADER: Title, Breadcrumbs, Action Bar                   |
+-------------------+--------------------------------------------------------------+
| (•) Recording     |                                                              |
| [>] Meetings (act)| MAIN WORKSPACE VIEW (Scrollable or Resizable Split Pane)     |
| [?] Knowledge Base|                                                              |
| [M] Model Manager |                                                              |
| [*] Settings      |                                                              |
|                   |                                                              |
|                   |                                                              |
+-------------------+                                                              |
| [• Offline Local] |                                                              |
| Download: 45% ... |                                                              |
+-------------------+--------------------------------------------------------------+
```

- **Sidebar Width**: 240px fixed in expanded mode; can collapse to a 60px icon-only rail for small screens.
- **Active Navigation Indicator**: A subtle 32px-height pill filled with `--color-green-tint` (`#E2F3EB` light / `#173629` dark) with a 3px vertical accent bar on the left edge in `--color-primary` (`#419873`).
- **Footer Diagnostics**:
  - Offline status pill: `• Offline · Local Engine Active` with a subtle pulsing green dot.
  - Background model download bar: Progress indicator with percentage, estimated time remaining, and click-to-open Model Manager.

---

### 4.2 Surface 1: Meeting Detail Studio (`/meeting/:id`) — North-Star View

This view represents the central workspace where users review, play, search, and synthesize meeting data.

```
+----------------------------------------------------------------------------------+
| Q3 Architecture & Offline RAG Sync                                                |
| Sep 10, 2026 · 42:15 · 3 Speakers · Type: Engineering Sync    [Copy] [Export] [Re-run]|
+------------------------------------+---------------------------------------------+
| [ VIDEO / SCREEN PLAYER ]          | [ TABS: Transcript* | Slides | Summary | Actions | Chat ] |
|                                    +---------------------------------------------+
| +--------------------------------+ | 14:28  [Speaker 1]                          |
| | Screen Share Diagram           | | Speaker 1: The architecture aligns with ... |
| |                                | |                                             |
| |                                | | 14:32  [Speaker 1] <ACTIVE LINE>            |
| +--------------------------------+ | +-----------------------------------------+ |
| |> 14:32 / 42:15  [=====•====]  [] | | | Speaker 1: We converted the sidecar to| |
| +--------------------------------+ | | | an on-demand process with ChromaDB... | |
| Auto-extracted Slides (6)          | +-----------------------------------------+ |
| [<] [02:10] [08:45] [14:30]* [22:15]| 14:40  [Speaker 2]                          |
|                                    | Speaker 2: Does that run on CPU fallback?   |
+------------------------------------+---------------------------------------------+
```

#### Anatomical Sub-Components

##### A. Synchronized Video Player
- **Engine**: Selected player (Plyr or Video.js) wrapped in custom Tauri-optimized chrome.
- **Aspect Ratio**: 16:9 responsive container with sharp 6px border radius and 1px border (`--color-border`). For audio-only recordings, renders a dedicated audio transport card with subtle waveform graphic rather than a blank or dummy video frame.
- **Media Timeline Bar**:
  - Scrubber track: 4px height (`--color-surface-sunken`).
  - Progress fill: `--color-primary` (`#419873`).
  - Active hover thumb: 12px circular handle with luminous mint border.
  - Controls: Play/Pause, Seek forward/back 5s, Tabular Elapsed/Total Time (`14:32 / 42:15`), Speed Selector (`0.75x`, `1.0x`, `1.25x`, `1.5x`, `2.0x`), Volume slider with mute toggle, Fullscreen.

##### B. Auto-Extracted Slides Strip & Dedicated Slides Tab (PRD FR7.6)
- **Preview Strip Position**: Directly beneath the video player for rapid visual scrubbing.
- **Dedicated Slides Tab**: Full tab in the right-hand panel alongside Transcript, Summary, Actions, and Chat.
- **Format & Content**:
  - Gallery of extracted slide cards detected via OpenCV frame differencing.
  - Each slide card features a high-resolution preview thumbnail, primary transition timestamp badge (e.g. `[14:30]`), slide sequence label (`Slide 4 of 8`), and OCR text snippet.
  - Repeated occurrences: When a presenter returns to a previously shown slide, repeated timestamp chips (e.g. `also at 28:15`) appear on the card.
- **Interactions**:
  - Clicking any slide or occurrence immediately seeks the video player to that exact timestamp (<200ms latency) and synchronizes the transcript view.
  - Active slide indicator: A 2px solid `--color-primary` frame highlights the slide corresponding to the current playback position.
  - Audio-only or no-slides recordings display an explicit informational empty state: `"No presentation slides in this recording"`.
  - Processing failure: If slide extraction or OCR encounters an error, an inline banner provides error details and a retry button without impairing access to video, audio, or transcript.

##### C. Synchronized Transcript Engine
- **Layout**: Virtualized vertical list handling up to 3 hours of speech without DOM degradation.
- **Segment Item Anatomy**:
  - Left gutter: Tabular timestamp (`14:32`) in `--color-ink-muted`. Clicking seeks media player.
  - Speaker Badge: Rounded pill with speaker label (e.g. `Speaker 1` or renamed `Morgan`), styled with that speaker's unique diarization hue.
  - Transcript Body: High-contrast ink text (`--color-ink`) with 1.5 line height.
- **Active Segment Indication**:
  - When media playback enters a segment's time range `[start, end]`:
    1. A 3px vertical accent bar in `--color-mint` (`#52bf90`) renders along the left edge.
    2. The segment background gently transitions to `--color-surface` (`#F3EFEA`).
    3. The transcript container smoothly scrolls to keep the active segment in the vertical center.
    4. If the user manually scrolls the transcript, auto-scrolling pauses for 8 seconds and displays a floating `"Jump to current"` pill button.

##### D. AI Summary & Action Items Tabs
- **Summary Tab**:
  - Executive Overview paragraph, structured Key Decisions bullet list, and Key Concepts.
  - Direct citations: Every factual claim includes a clickable timestamp footnote (e.g. `[14:32]`) that seeks media and highlights the transcript/slide evidence.
  - Outdated result indicator: When source inputs (audio edit, re-transcription, meeting type override) change, an amber status banner appears (`Source updated · Summary outdated`), retaining previous summary text until the user explicitly requests regeneration.
  - Summary revision selector: Dropdown/picker allows reviewing historical summary revisions with timestamp and model provenance.
- **Action Items Tab**:
  - Checkable task cards with native checkbox affordance.
  - Assignee badge (linked to speaker ID) and extracted deadline pill (e.g. `Due: Friday, Sep 15`). Unknown assignees or deadlines remain explicitly labeled as unknown/unspecified.
  - Revision-owned completion state: Checking an item strikes through text and updates SQLite state immediately for that specific summary revision (`summary_revision_id`). When an existing summary is explicitly regenerated, earlier revisions and their completed items are preserved; new revisions do not steal or reset old completion records, nor automatically transfer completion by text matching.

##### E. In-Meeting Scoped Q&A Drawer (Chat Tab)
- Input field at bottom: `"Ask about this meeting..."`
- Fixed chat scope: Restricted to this meeting's transcript, extracted slide OCR, current summary, and linked documents.
- Streaming responses render markdown with navigable citations.
- Incomplete turns: If generation is canceled or interrupted, the turn is permanently labeled `[Incomplete Response]` with a retry button.

---

### 4.3 Surface 2: Core Capture & Recording Hub (`/record`)

```
+----------------------------------------------------------------------------------+
| Start a New Recording                                                            |
| Configure your capture sources and meeting classification                        |
+----------------------------------------------------------------------------------+
| Meeting Title: [ Q3 Architecture & Offline RAG Sync                          ]   |
| Meeting Type:  (•) Auto-detect    ( ) Business Meeting    ( ) Lecture            |
+----------------------------------------------------------------------------------+
| CAPTURE SOURCES                                                                  |
| +-------------------------+ +-------------------------+ +----------------------+ |
| | [x] System Audio        | | [ ] Microphone          | | [x] Screen Video     | |
| | Default Output Device   | | MacBook Pro Mic         | | Entire Screen 1      | |
| | Level: [====    ] -12dB | | Level: [      ] -inf dB | | 1920x1080 @ 30fps    | |
| +-------------------------+ +-------------------------+ +----------------------+ |
| [ ] Only me on this microphone (assigns microphone speech to User)               |
+----------------------------------------------------------------------------------+
|                                                                                  |
|                        [ (•) START RECORDING ]                                   |
|                                                                                  |
|           • Capture priority mode active · Disk space: 84.2 GB free              |
+----------------------------------------------------------------------------------+
```

- **Capture Defaults (PRD FR1.5)**: System Audio (ON) + Screen (ON) + Microphone (OFF). At least one audio source is strictly required to start recording; all-off or screen-only setup is rejected with an actionable validation message.
- **Explicit Personal Microphone Setting (PRD FR12.2)**: "Only me on this microphone" checkbox toggle (default OFF). When disabled, pyannote diarizes microphone speech alongside system audio without presuming identity. When enabled, microphone speech is assigned to the User persona.
- **Recording Initiation**: ≤2 interactions required from launch once OS permissions and source choices are established.

#### Active Recording State (Live HUD)
When recording begins:
- Global window title bar and sidebar header switch to an active recording mode.
- Pulsing red recording beacon (`0.5Hz` calm pulse).
- Large live tabular elapsed timer (`00:14:32`).
- Live audio visualizer meters displaying discrete VU levels for system audio and microphone channels.
- Transport buttons: `[ Pause ]`, `[ Resume ]`, `[ Stop & Process ]`.
- 3-hour recording warning: At 3 hours, a polite non-blocking warning banner appears (`Session reached 3 hours · capture continues`), without interrupting recording.
- **Window Close Protection & Trayless Fallback**:
  - Closing the main window during recording minimizes to the OS system tray with an unobtrusive desktop notification and keeps recording alive without data loss.
  - If a supported desktop environment lacks system tray support (e.g. certain Linux Wayland compositors), Locus retains an accessible, compact recording-control window with timer, audio meters, pause/resume, and stop controls so recording is never hidden without a discoverable control surface.
  - Explicit application quit during active capture displays an authoritative confirmation modal: `[ Stop and save ]` or `[ Cancel ]`.

---

### 4.4 Surface 3: Knowledge Base & Multi-Scope RAG (`/knowledge`)

```
+-------------------+--------------------------------------------------------------+
| CONVERSATIONS     | CHAT SCOPE: [ All Meetings v ]   MODEL: [ Llama 3.2 3B (Local) v]|
| + New Thread      +--------------------------------------------------------------+
|                   | USER: What decisions were made regarding the sidecar memory? |
| Recent:           |                                                              |
| • Q3 Sync RAG     | LOCUS: In the Q3 Architecture Sync (Sep 10), the team decided|
| • Pipeline Design | to convert the Python sidecar into an on-demand process to   |
| • Lecture Notes   | bound RAM usage [1]. ChromaDB vector indexing will run as a  |
|                   | background job rather than keeping PyTorch loaded [2].       |
| DOCUMENTS (12)    |                                                              |
| + Ingest File     | Sources & Citations:                                         |
| • arch_v2.pdf     | [1] Q3 Architecture Sync @ 14:32 (Jump to transcript)        |
| • spec_draft.md   | [2] arch_v2.pdf, Page 4 (Open document excerpt)              |
|                   +--------------------------------------------------------------+
| Index: Ready [✓]  | [ Ask a question across your meeting library...         ][Send]|
+-------------------+--------------------------------------------------------------+
```

- **Fixed Scope Selector**: Dropdown pill allowing the user to set the retrieval scope for the thread:
  1. `This meeting only`: Includes the meeting's transcript, extracted slide OCR, current summary, and linked documents (requires a selected meeting).
  2. `All meetings`: Searches transcripts, slide text, and summaries across all meetings (excludes independent documents).
  3. `Documents only`: Searches uploaded PDFs, Markdown, and text files (excludes meeting transcripts).
  4. `Everything`: Global Knowledge Base combining all meetings, linked documents, and independent files.
  - *Scope Invariant*: Chat threads have a fixed scope. Changing the scope creates a new thread and preserves the previous thread without inheriting context.
- **Model Destination Transparency & Provider Selection**:
  - Saving API keys or configuring Ollama makes providers available; explicit selection in the model dropdown authorizes generation at that destination.
  - Before selection, the UI discloses what data will be sent (meeting text for summaries/titles; user question, conversation history, and retrieved excerpts for chat).
  - Selected destinations are clearly badged:
    * Local Llama: green shield badge `Local · Private (Offline)`.
    * Remote Provider: globe badge `Cloud · OpenAI (gpt-4o) · Disclosed Destination`.
  - Selection persists until changed; no redundant selection prompt is shown on every message.
- **Navigable Citations & Evidence**:
  - Answers cite exact transcript timestamps, slide times, or document page numbers.
  - If sources contain insufficient evidence, the assistant explicitly states inability to answer rather than fabricating facts.
  - Canceled or failed stream turns are labeled `[Incomplete Response]` with retry affordance.

---

### 4.5 Surface 4: Unified Model Manager (`/models`)

- **Categorized Tabs**:
  1. **Transcription Models**: Bundled Whisper `small` (466 MiB, read-only), downloadable `tiny`, `base`, `medium`, `large-v3`, and quantized variants (`q5_0`, `q8_0`).
  2. **Summarization Engines**: Bundled `llama-server`, curated GGUF models (Llama-3.2 3B, Mistral 7B, Qwen 2.5) with measured RAM/VRAM requirements and quality tradeoffs.
  3. **Embedding Models**: Local GGUF embedding models for ChromaDB semantic search and RAG.
- **Model Card Anatomy**:
  - Name, parameter size, quantization level, and primary author.
  - Memory Requirements: Minimum system RAM and recommended GPU VRAM.
  - Disk Footprint: Measured download size and on-disk size.
  - Status Pills: `Bundled`, `Installed`, `Active`, `Downloading (45%)`, `Available`, `Corrupt / Verification Failed`.
  - Actions: `Download`, `Import Local File (.gguf)`, `Set as Active`, `Delete` (with explanation if model is currently active/in use).
- **Skippable Setup**: First-launch model setup can be skipped without blocking recording or basic transcription. Unprovisioned models show clear download prompts on dependent features without disabling independent capabilities.

---

### 4.6 Comprehensive Screen & State Mapping (Five Sidebar Routes + Meeting Detail)

The table below maps the complete set of views and their primary states across the five sidebar routes plus Meeting Detail Studio:

| Route / Surface | Normal State | Empty State | Loading / In-Progress State | Failure / Degraded / Error State |
|---|---|---|---|---|
| **`/record`**<br>(Capture Hub) | Source selectors (System Audio ON, Screen ON, Mic OFF), "Only me on mic" toggle, title input, meeting type selector, disk space indicator, Start Recording button. | Initial first-launch view: default sources pre-selected, ready to record in ≤2 clicks. | **Recording Active (Live HUD)**: elapsed timer, pulsing beacon, audio VU meters, Pause/Resume, Stop & Process. | **Validation Error**: all audio sources off (`"At least one audio source required"`).<br>**Permission Error**: OS mic/screen recording denied with link to System Settings.<br>**Disk Exhaustion**: disk space < 1 GB triggers safe auto-stop preserving captured media.<br>**Recovery State**: see below. |
| **`/`**<br>(Meeting Library) | Paginated list of meetings with title, date, duration, speaker count, type badge, processing status, search bar, date/type filters. | `"No recordings yet"` with illustration and prominent `[ Start a Recording ]` CTA button. | Skeleton list rows with pulsing placeholders while SQLite query executes. Background pipeline progress indicator on running meetings (`"Transcribing 65%"`). | **Pipeline Step Error**: failed step badge (`"OCR Failed"`) with error message and `[ Retry ]` button; successful upstream artifacts remain fully accessible.<br>**Storage Inaccessible**: alert banner if configured external meeting storage drive is disconnected. |
| **`/meeting/:id`**<br>(Meeting Detail) | Split layout: synchronized media player + slide strip on left; tabbed panel (Transcript, Slides, Summary, Actions, Chat) on right. | For audio-only recording: dedicated audio transport card; Slides tab shows `"No slides in this recording"`. | Media buffering spinner; summary generation shimmering placeholder with step timer (`"Generating summary… 14s"`). | **Playback Error**: media decode error with fallback audio-only or file link.<br>**Outdated Result**: amber banner `"Source updated · Summary outdated"` with `[ Regenerate Summary ]` button.<br>**Regeneration Error**: regeneration failure retains prior summary revision without data loss. |
| **`/knowledge`**<br>(Knowledge Base) | Left: conversation threads + document list + upload button. Right: chat thread, scope selector, model badge, cited responses, input composer. | `"No conversations yet"` with suggested prompt starters (`"Summarize key decisions from last week"`). Document list shows `"Drop PDFs or text files here"`. | Streaming chat response with pulsing stop button; document upload progress bar with per-file status (`"Extracting text…"` / `"Indexing vectors…"`). | **Upload Error**: rejected encrypted/scanned/oversized file with specific rationale.<br>**Incomplete Chat**: network/stream interruption stamped `[Incomplete Response]` with `[ Retry ]` button.<br>**Embedding Model Missing**: banner explaining semantic search requires embedding model with `[ Provision Model ]` CTA. |
| **`/models`**<br>(Model Manager) | Tabbed catalog of Whisper, LLM, and Embedding models showing size, memory requirements, installation status, active badges. | N/A (catalog is populated by bundled models and known local variants). | Download progress bar with percentage, download speed, and `[ Cancel ]` button. Hash verification spinner upon completion. | **Download Failed**: actionable retry button with network error message.<br>**Corrupt File**: checksum mismatch alert offering re-download.<br>**In-Use Deletion Guard**: explains active model cannot be deleted until an alternative is selected. |
| **`/settings`**<br>(Settings) | Grouped sections: General, Appearance (Light/Dark/System), Recording Defaults, LLM Providers (credentials, endpoint URL), Storage & Migration, Privacy. | N/A (form controls display defaults). | Relocation progress modal (`"Copying meeting data… 42%"`), verifying files before switching storage pointer. | **Keychain Error**: locked or unavailable OS keychain explained clearly; no plaintext fallback.<br>**Storage Move Blocked**: active capture/job blocks relocation with reason.<br>**Move Failed**: migration rolls back cleanly to original storage directory. |

#### Detailed Critical State Specifications

##### 1. Recording Recovery State (PRD FR12.3, SPEC §4)
- **Trigger**: Application launch after unexpected process crash, power loss, or forced OS shutdown during active capture.
- **Presentation**: An authoritative modal dialog appears before any new recording can begin:
  - Header: `"Interrupted Recording Detected"`
  - Metadata: Date/time of interrupted session, recoverable duration, and estimated lost tail (target loss ≤ 5 seconds).
  - Actions:
    1. `[ Recover and Process ]` (Primary Evergreen): Closes the captured media container, creates a durable Meeting record in SQLite, and queues background processing (VAD, transcription, diarization).
    2. `[ Discard Recording ]` (Danger Outline): Permanently purges the orphaned temporary media files and removes the recovery lock.
  - Invariant: Repeated recovery attempts or modal re-opens never create duplicate meeting entries.

##### 2. Outdated Results & Summary Revision State (PRD FR12.6, SPEC §4)
- **Trigger**: Upstream edits occurring after initial summary generation (e.g. user edits transcript, speaker renames, or meeting type override).
- **Presentation**:
  - An amber warning banner (`--color-status-warning`) docks above the summary text:
    `"Source content was updated · Current summary is outdated"`
  - Controls: `[ Keep Current Revision ]` and `[ Regenerate Summary ]`.
  - Revision Preservation: Generating a new summary creates a new immutable `SummaryRevision` record. Previous revisions remain selectable via a revision history picker (`v1 (Original)`, `v2 (Regenerated)`).
  - Action Items Protection: Action item checkboxes belong to their specific summary revision (`summary_revision_id`). Checking an item in v1 preserves its completed state; v2 generates fresh action items without overwriting or inheriting v1 states.

##### 3. Missing / Skipped Models State (PRD FR12.14, FR10.4)
- **Trigger**: User skips first-launch model download, or attempts summarization/RAG when local GGUF weights are uninstalled.
- **Presentation**:
  - Independent features remain fully operational (recording, playback, transcription with bundled Whisper `small` continue uninterrupted).
  - Summary tab displays a clean inline card:
    - `"Local Summarization Engine Not Installed"`
    - Subtext: `"Download a recommended 3B model (approx. 2.1 GB) or configure a remote LLM provider in Settings."`
    - Action buttons: `[ Download Recommended Model ]` and `[ Configure Remote Provider ]`.
  - Knowledge Base displays an inline banner:
    - `"Semantic Search Requires Local Embedding Model"`
    - Action button: `[ Download Embedding Model (350 MB) ]`.

##### 4. Incomplete Chat & Stream Interruption State (SPEC §4)
- **Trigger**: User clicks `[ Stop ]` during chat streaming, local sidecar runs out of context memory, or remote API connection drops.
- **Presentation**:
  - The partial text already received is retained verbatim in the message bubble.
  - An inline status badge appears at the bottom of the partial turn: `[Incomplete Response — Generation Interrupted]`.
  - An actionable `[ Resume / Retry ]` button is rendered immediately beneath the turn.
  - The turn is never presented as a complete answer, and subsequent questions in the thread retain awareness of the incomplete turn.

---

## 5. Component Library & Implementation Tokens

The frontend builds on **Astryx** (Meta's component design system) styled via **TailwindCSS**.

### 5.1 Interactive Controls & States

#### Buttons
- **Primary Action Button**:
  - Fill: `--color-primary` (`#419873`)
  - Text: Pure white (`#FFFFFF`) with font-weight 600
  - Radius: 6px
  - Hover: `--color-primary-hover` (`#388463`)
  - Focus Ring: 2px solid `--color-accent` with 2px offset
  - Active: Scale `0.98` with 100ms transition
- **Secondary / Outline Button**:
  - Fill: Transparent
  - Border: 1px solid `--color-border` (`#E5E0D6` light / `#28352F` dark)
  - Text: `--color-ink` (`#19211D` light / `#F0F3F1` dark)
  - Hover: Fill `--color-surface` (`#F3EFEA`)
- **Danger Button**:
  - Fill: `--color-status-error` (`#C53030`)
  - Text: `#FFFFFF`

#### Form Controls
- **Text Inputs & Search Bars**:
  - Background: `--color-surface-elevated`
  - Border: 1px solid `--color-border`
  - Focus: 1px solid `--color-primary` + 2px box-shadow in `oklch(0.612 0.118 158.0 / 0.2)`
  - Text: `--color-ink` with placeholder in `--color-ink-subtle`
- **Toggle Switches** (Capture Sources):
  - Track (Inactive): 36px × 20px, filled with `--color-border`
  - Track (Active): Filled with `--color-primary` (`#419873`)
  - Handle: 16px white circle with 1px drop shadow, slides with 150ms ease-out.

---

## 6. Interaction Design & Motion Grammar

### 6.1 Duration & Easing Standards
All transitions use purposeful, mechanical easing:
- **Fast / Micro-interactions** (Button hover, checkbox toggle, tab switch): `150ms cubic-bezier(0.16, 1, 0.3, 1)`
- **Standard Panel Transitions** (Sidebar collapse, drawer open, modal reveal): `220ms cubic-bezier(0.16, 1, 0.3, 1)`
- **Continuous State Indicators** (Recording pulse beacon): `2000ms infinite ease-in-out`

### 6.2 Keyboard Shortcuts Matrix

Locus provides comprehensive desktop keyboard navigation:

| Key Binding | Action | Scope |
|---|---|---|
| `Space` | Play / Pause Video Playback | Meeting Detail |
| `Left Arrow` / `Right Arrow` | Seek ±5 seconds | Meeting Detail |
| `Shift + Left` / `Shift + Right` | Seek ±15 seconds | Meeting Detail |
| `J` / `K` | Previous / Next transcript segment | Meeting Detail |
| `C` | Copy formatted summary to clipboard | Meeting Detail |
| `Cmd/Ctrl + K` | Global search across meetings & knowledge base | Global |
| `Cmd/Ctrl + N` or `Cmd/Ctrl + R` | Start / Open New Recording | Global |
| `Cmd/Ctrl + ,` | Open Settings | Global |
| `Esc` | Close modal / exit fullscreen / clear selection | Global |

---

## 7. Accessibility & Usability Compliance

1. **WCAG 2.1 AA / AAA Compliance**:
   - Body copy contrast: **13.8:1** (Light), **14.5:1** (Dark).
   - Secondary muted text: **4.8:1** (Light), **6.1:1** (Dark).
   - Interactive button fills: Tested for perceptual contrast with white text.
2. **Keyboard Focus Affordance**:
   - All interactive elements exhibit an unambiguous `:focus-visible` ring (`2px solid var(--color-accent)` with `2px` offset).
3. **Screen Reader Semantics**:
   - ARIA live regions (`aria-live="polite"`) announce live recording timer updates and background transcription job status without interrupting user focus.
   - Video player and transcript synchronization maintain synchronized `aria-current="time"` attributes on active segments.
4. **Resilience to OS Preferences**:
   - `prefers-reduced-motion`: Disables pulse animations and replaces smooth scrolling with instantaneous pagination.
   - `prefers-color-scheme`: Automatically selects Light or Dark theme when configured to System.

---

## 8. Frontend Assets & Mockup Provenance

| Asset Name | Relative Path | Provenance / Generator | Role |
|---|---|---|---|
| **Meeting Detail North-Star Comp** | `.impeccable/mocks/meeting-detail-comp.jpg` | Antigravity `generate_image` (Seed: 1788971893764) | Approved visual benchmark for `/meeting/:id` |
| **Meeting Detail Prompt Sidecar** | `.impeccable/mocks/meeting-detail-comp.json` | Impeccable prompt provenance record | Build audit contract |
| **Astryx Component Library** | `src/components/ui/` | Native React 19 + Astryx design tokens | Base UI primitive components |
