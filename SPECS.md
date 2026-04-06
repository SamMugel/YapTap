# YapTap — Specification Overview

YapTap is an AI writing assistant that records the user's voice, transcribes it, combines it with a pre-defined prompt, and outputs polished text directly at the cursor.

## Authoritative Feature Reference

This file and the spec documents it links to are the authoritative feature reference. No feature may be coded unless it is described here or in a linked spec.

Implementation tasks are tracked in `PRD/`, one file per phase.

## Spec Documents

| Document | Description |
|---|---|
| [specs/roadmap.md](specs/roadmap.md) | Phased delivery plan (phase 1 detailed, phases 2–4 high-level) |
| [specs/architecture.md](specs/architecture.md) | System architecture, language boundaries, component overview |
| [specs/cli.md](specs/cli.md) | CLI interface specification for `yaptap` (phase 1) |
| [specs/audio-recording.md](specs/audio-recording.md) | Microphone capture: format, device selection, lifecycle |
| [specs/transcription.md](specs/transcription.md) | Whisper transcription via Python subprocess |
