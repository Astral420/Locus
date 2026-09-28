# Locus — Frontend Design & Visual System Specification

> **Source of Truth**: This document details the visual identity, design tokens, layout topology, and interaction specifications for the Locus Tauri frontend (React + TypeScript).  
> For the root design system file used by Impeccable, see [DESIGN.md](../../DESIGN.md).  
> For system architecture, Rust backend orchestration, and sidecar boundaries, see [DESIGN.md](./DESIGN.md).  
> For observable behavior and acceptance criteria, see [SPEC.md](./SPEC.md).  
> For product requirements, see [PRD.md](./PRD.md).

---

## Executive Summary

Locus is designed around the metaphor of a **Precision Master Control Room & Archival Research Library**. It provides a calm, tactile, distraction-free environment for recording, reviewing, searching, and managing high-value meeting knowledge.

This specification establishes:
1. **The Editorial Botanical Palette**: Resolving the user's color proposal (`#419873`, `#49ab81`, `#52bf90`) into accessible OKLCH design tokens with deep pine-charcoal typography (`#19211D`) on warm architectural paper-beige (`#FAF8F5`) in Light Mode and obsidian slate (`#111614`) in Dark Mode.
2. **The Meeting Detail Studio**: Full spatial, typographic, and behavioral specifications for video/audio playback, auto-extracted slide strip and required **Slides tab** (FR7.6), timestamped transcript highlighting, AI summary revisions, and revision-owned action item completion.
3. **Core Capture HUD & Knowledge Base Chat**: Ergonomics for ≤2-click recording with default System Audio (ON) + Screen (ON) + Microphone (OFF), explicit "Only me on this microphone" personal setting, background recording persistence with trayless control-window fallback, and cited multi-scope Knowledge Base chat (with meeting scope including transcripts, slide text, current summary, and linked documents).
4. **Provider Transparency & Error States**: Explicit provider selection destination disclosure without per-request prompts, structured recording recovery for interrupted sessions, clear outdated-result indicators, and graceful missing-model and incomplete-chat flows.
5. **Astryx & Tailwind Component Guidelines**: Accessibility, keyboard bindings, and motion standards.

Please refer to the comprehensive root specification in [DESIGN.md](../../DESIGN.md) for complete token tables, screen/state mappings, and interaction contracts.
