# Decisions

Each entry: date · decision · why · how to reverse. The agent adds to this file; it does not
reopen an entry without a new reason.

- **2026-11-17 · Oblivion is the only engine; NEXUS moves to `legacy/`.** NEXUS returns canned
  text and has security holes S1–S3; keeping two engines doubles all work. Reverse: `git mv`
  `legacy/nexus` back and restore the adapter import in `web/app.py`.
- **2026-11-17 · The desktop moves from Flask into Oblivion (axum) in month 3.** Needed for a
  one-file install without Python. Flask stays until then. Reverse: keep `web/` and the bridge.
- **2026-11-17 · Names: "Jarvis" is the desktop, "Oblivion" is the engine.**
- **2026-11-17 · Privacy default.** File contents may go to cloud models only when a cloud key
  is configured; the "Offline" switch forces local models only.
- **2026-11-17 · Autopilot.** The coding agent works through `PROGRESS.md` without asking,
  within the limits in `.agents/rules/autopilot.md`. Merges to `main` only at milestones.
- **2026-11-19 · Desktop template and static assets embedded directly in Axum binary with rust-embed.** Eliminates runtime Python or Flask requirement for the executive desktop interface. Zero external file dependencies for web serving; secret token injected into embedded HTML at runtime; classic chat remains accessible via `/classic`. Reverse: remove `rust-embed` and revert `page` handler in `crates/oblivion-cli/src/web.rs`.
- **2026-11-20 · First-run onboarding wizard and autostart integration with WiX installer.** First-run wizard manages workspace selection, model privacy mode (local vs hybrid), and Windows autostart (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`), accessible via CLI (`--wizard`, `--autostart`), REST endpoints, and desktop UI modal. Packaging pipeline in `installer/build_installer.ps1` builds release executable, packages portable zip, and generates WiX MSI specification; code signing marked BLOCKED (needs user) for certificate provision. Reverse: remove `autostart.rs`, `wizard.rs`, and `installer/` directory.
- **2026-11-20 · Voice subsystem with push-to-talk, audio buffer WAV parsing, and local/cloud STT.** Push-to-talk hotkey (`Ctrl+Space`) and mic buttons in menubar (`#menubar-mic-btn`) and Oblivion panel (`#ob-mic-btn`) with pulsing recording visual states. Audio capture via Web Speech API or `getUserMedia` -> `/api/oblivion/voice/transcribe`. Backend `VoiceService` routes between `LocalWhisperTranscriber` (via `whisper-cli`/`whisper.exe`), `CloudWhisperTranscriber` (Groq/OpenAI Whisper when configured and not offline), and pure-Rust 16kHz mono WAV audio parsing. Reverse: remove `voice.rs` and voice REST endpoints.
