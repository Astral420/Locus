# Ship the patched llama-server as a separate binary, with its source offered

Locus bundles a ToshLLM-patched `llama-server` on macOS x86_64. ToshLLM's patch series is
GPL-3.0-or-later (`LICENSE`, `patches/COPYRIGHT`, `AUTHORS.md`) and the patched binary is a
derivative of llama.cpp (MIT) plus those patches, so that binary is distributed under GPL terms.

- **Process boundary.** Locus starts `llama-server` as its own process and talks to it over loopback
  HTTP; nothing from the patched sources is linked into Locus, as with the GPL FFmpeg build already
  shipped as a separate executable.
- **What ships.** `packaging/llama-engine/build.sh` stages the binary, its precompiled Metal kernels,
  `LICENSE-GPL-3.0.txt`, `engine.json` (both pinned commits) and `SHA256SUMS` into the app resources,
  macOS x86_64 only. Locus' own `LICENSE` is not part of this decision and does not change.
- **What is offered.** `packaging/llama-engine/source-offer.sh` builds a deterministic
  `llama-engine-source-<tag>.tar.gz` — the patch series and `scripts/build-engines.sh` at the pinned
  commit, the notices, `pin.env`, and a generated `BUILDING.md` and `SOURCE-OFFER.txt` — and the
  release workflow attaches it beside the installer.
- **Whisper is untouched.** Linking patched code in-process — for example patched whisper through
  `whisper-rs` — would likely make the whole application a GPL derivative. Locus does not do that.
- **The owner decides.** Whether that process boundary is a sufficient answer for Locus, and what
  Locus' own licence should be, is the owner's call with counsel; this ADR records facts only.

Related: [third-party notices](../release/THIRD_PARTY_NOTICES.md).
