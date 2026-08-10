# tinyviolin implementation plan

## Goal

Create a small Rust library crate named `tinyviolin` that generates useful basic synthesized sounds, can be driven by sample-timed events or compact MIDI messages, and is safe to call from a real-time audio callback without allocation or locking.

The repository is currently empty apart from Git metadata, so the crate, tests, documentation, examples, and CI will all be introduced by this plan.

## Scope

### Included

- Mono `f32` block synthesis; the caller supplies the output slice, whose length is the current buffer size.
- Sample-rate configuration outside the audio callback and sample-offset events within each buffer.
- Presets for sine, square, triangle, bass, pad, lead, bass drum, tom, snare, and hi-hat.
- Fixed-capacity polyphony with deterministic voice allocation/stealing and no processing-time heap use.
- Core note-on, note-off, and all-notes-off events.
- A fixed-capacity MIDI 1.0 wrapper accepting self-contained messages of at most four bytes, including note-on, note-off, velocity-zero note-on, and all-notes/all-sound-off control changes.
- Fixed MIDI channel/note mappings to one or more instrument layers, with per-layer pitch behavior and gain.
- Documentation, deterministic tests, an audible render example, and GitHub Actions.

### Excluded from the initial crate

- Audio device or host integration, sequencing, file playback, effects, spatialization, and stereo mixing.
- SysEx, running-status streams, MIDI 2.0/UMP interpretation, sustain pedal, pitch bend, and arbitrary controller automation.
- Runtime patch graphs, dynamic voice counts, and sample loading.
- A promise of band-limited/alias-free oscillators or production-grade physical modeling.

## Immutable acceptance criteria

1. `tinyviolin` exposes documented instruments for `Sine`, `Square`, `Triangle`, `Bass`, `Pad`, `Lead`, `BassDrum`, `Tom`, `Snare`, and `HiHat`.
2. Every instrument is synthesized from phase accumulators, elementary waveforms, deterministic noise, and simple amplitude/frequency envelopes; no samples or runtime DSP dependencies are used.
3. The core processes caller-owned mono `&mut [f32]` buffers at a configured valid sample rate and applies ordered events at documented sample offsets.
4. Polyphony has a compile-time or construction-time fixed maximum, uses deterministic voice stealing when full, and behaves correctly for overlapping notes and percussion one-shots.
5. After construction and preparation of caller-owned event buffers, core event dispatch, MIDI dispatch, and audio processing perform zero heap allocations and acquire no locks. This is covered by an allocation-counting test and enforced structurally with fixed-size storage and safe Rust.
6. Core control supports note-on (instrument, frequency, gain/velocity, identity), note-off, and all-notes-off. Invalid frequencies, sample rates, and event offsets have documented, deterministic handling without panics in normal API use.
7. The MIDI wrapper accepts a length-tagged message backed by at most `[u8; 4]`, validates message length/data, handles the included MIDI messages, and does not allocate while translating or processing them.
8. MIDI mappings can address every channel/note pair and trigger one or more fixed-capacity layers. A layer can select an instrument, use equal-tempered MIDI pitch or a fixed frequency, and set gain. Note-off reliably releases voices originally started by the same channel/note.
9. Automated tests establish finite/bounded output, oscillator pitch, envelope and one-shot behavior, deterministic noise, sample-accurate event timing, voice stealing, MIDI parsing/mapping, malformed-input handling, and real-time allocation behavior.
10. A runnable example renders all presets to an auditionable WAV file, and the README explains integration, event timing, MIDI mapping, capacity limits, and real-time restrictions.
11. GitHub Actions runs formatting, linting, tests, docs, and builds covering x86_64, x86/i686, AArch64, ARMv7, and WebAssembly through these Rust targets: `x86_64-unknown-linux-gnu`, `i686-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `armv7-unknown-linux-gnueabihf`, `x86_64-pc-windows-msvc`, `i686-pc-windows-msvc`, `aarch64-pc-windows-msvc`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `wasm32-unknown-unknown`.
12. The final repository passes all documented local checks on stable Rust and the complete GitHub Actions workflow is green.

## Design rules and constraints

- Prefer a small, dependency-free safe-Rust implementation (`#![forbid(unsafe_code)]`) over abstraction or configurability not required above.
- Keep the audio path bounded and predictable: fixed arrays, no `Vec`, boxing, locks, I/O, logging, blocking, or lazy initialization during event/MIDI/audio processing.
- Infer block size from the output slice. Store sample rate and precomputed sample-rate-dependent values in engine/voice state; reconfiguration is not an audio-callback operation.
- Use a fixed voice pool. Give note events stable identities, define release semantics, and document the deterministic stealing priority (finished voices first, then released voices, then oldest active voice).
- Use straightforward category-defining DSP: phase-accumulator basic waves; small waveform mixes and preset envelopes for bass/pad/lead; pitch-decaying tone for bass drum and tom; tone-plus-noise for snare; short bright noise/metallic components for hi-hat. Use a per-voice deterministic PRNG rather than a system RNG.
- Keep exposed settings minimal: instrument, frequency/pitch mode, and gain. Preset envelope and mix constants remain internal unless evidence shows a required public control is missing.
- Use conservative per-voice levels and a final finite bounded output policy so polyphony cannot emit NaN/infinity or values outside the documented range.
- Represent MIDI mappings with fixed direct-index channel/note storage and a small fixed layer count, making lookup constant-time and mutation explicitly a non-callback setup operation.
- Keep pure DSP independent of an audio backend so the same library builds for native and `wasm32-unknown-unknown`.
- Prefer behavior/property tests over brittle full-buffer snapshots; use tight numeric fixtures only where they clarify parsing, timing, or deterministic DSP behavior.

## Staged implementation

Stages are ordered; each stage depends on the preceding stages unless explicitly noted.

### Stage 1 — Crate skeleton and API contract

- [x] Create `Cargo.toml`, `src/lib.rs`, module boundaries, crate-level documentation, formatting/lint configuration if needed, and a minimal README.
- [x] Define the public instrument/settings, engine-capacity, voice identity, timed event, process error, and MIDI mapping/message types without implementing full DSP.
- [x] Document mono output, event ordering/offset rules, valid sample rates/frequencies, output bounds, fixed capacities, and which methods are permitted in a real-time callback.
- [x] Add compile-time assertions/tests for compact copyable event/message types and forbid unsafe code.
- [x] Verify with `cargo fmt --check`, `cargo check --all-targets`, and `cargo doc --no-deps`.
- [x] Commit the completed skeleton and API contract.

### Stage 2 — DSP primitives and single-voice presets

- [x] Implement phase handling, sine/square/triangle generation, deterministic noise, and simple attack/decay/sustain/release or one-shot envelopes as private primitives.
- [x] Implement the ten instrument presets using the minimum category-defining mixes and pitch/amplitude modulation described in the design rules.
- [x] Ensure phase, envelope, PRNG, and frequency state are initialized before processing and reset deterministically when a voice is reused.
- [x] Add unit tests for waveform range/pitch, envelope transitions, percussion decay/pitch drop, deterministic noise, silence after completion, and non-finite input rejection.
- [x] Verify with formatting, linting, unit tests, and a release-mode test run.
- [x] Commit the DSP primitives and preset milestone.

### Stage 3 — Polyphonic event-driven engine

- [x] Implement the fixed voice pool, voice lifecycle, stable note identities, note release, all-notes-off, and deterministic voice stealing.
- [x] Implement block processing that clears/fills the caller buffer, splits work at ordered event offsets without temporary allocation, mixes active voices, and applies the documented output bound.
- [x] Handle simultaneous events, zero-length buffers, block-edge events, percussion one-shots, repeated identities, and full-pool behavior deterministically.
- [x] Add integration tests for exact event timing, overlapping notes, release tails, pool exhaustion/stealing order, varying buffer lengths, and consistency across block boundaries.
- [x] Add a dedicated counting-allocator test proving zero allocations during prepared core event dispatch and processing.
- [x] Verify all core tests in debug and release modes and inspect the processing call graph for forbidden operations.
- [x] Commit the polyphonic engine milestone.

### Stage 4 — Fixed-capacity MIDI wrapper

- [ ] Implement validated length-tagged MIDI messages with a four-byte maximum and parsing for the in-scope MIDI 1.0 channel messages.
- [ ] Implement direct channel/note mapping storage, fixed layering, equal-tempered or fixed-frequency pitch selection, gain/velocity handling, and setup-time mapping mutation APIs.
- [ ] Connect MIDI note lifecycle to stable `(channel, note)` identities so layered note-offs and all-notes/all-sound-off release the correct voices.
- [ ] Provide timed MIDI block processing without constructing an intermediate heap collection.
- [ ] Test every included status, all channels, representative note boundaries/velocities, layers, remapping behavior, malformed/unsupported data, event offsets, and pool overflow.
- [ ] Extend the counting-allocator test to MIDI parsing, lookup, dispatch, and processing.
- [ ] Verify formatting, linting, all tests, and API documentation; commit the MIDI wrapper milestone.

### Stage 5 — Usage documentation and audible validation

- [ ] Expand the README with minimal core and MIDI examples, capacity sizing, sample-rate setup, event-order requirements, mapping setup, and callback safety guidance.
- [ ] Add a dependency-free example that schedules melodic and percussion events and writes a valid WAV containing an identifiable section for each preset.
- [ ] Add rustdoc examples for engine construction, direct events, layered MIDI mapping, and error handling.
- [ ] Run the example, inspect WAV metadata/duration and sample bounds programmatically, and audition it using a documented checklist: stable basic pitches, bass low/rounded, pad slow/sustained, lead prominent, bass drum low with downward pitch, tom tonal, snare noisy/tonal, and hi-hat short/bright.
- [ ] Verify `cargo test --all-targets`, doctests, and a clean release example run; commit documentation and audible validation.

### Stage 6 — Cross-platform GitHub Actions

- [ ] Add a workflow for `cargo fmt --check`, strict Clippy, tests/doctests, and documentation on stable Rust.
- [ ] Add build jobs for the exact native and WebAssembly targets in acceptance criterion 11, using native hosted runners where useful and target builds for the remaining pure-library artifacts.
- [ ] Cache Cargo data without relying on generated platform artifacts, pin actions to maintained major versions, and keep workflow permissions minimal.
- [ ] Reproduce the workflow commands locally where the host permits and verify every matrix entry in GitHub Actions.
- [ ] Fix only portability defects; do not conditionalize away instruments, MIDI behavior, or tests on supported targets.
- [ ] Commit the green CI milestone.

### Stage 7 — Final end-to-end validation

- [ ] Start from a clean checkout and run formatting, strict linting, all targets/tests/doctests, docs, release builds, the allocation tests, and the WAV render example.
- [ ] Recheck every immutable acceptance criterion against code, tests, documentation, rendered output, and CI evidence; record the evidence in the final change summary.
- [ ] Confirm public docs contain no unimplemented promises, the dependency tree remains empty at runtime, and no audio/MIDI call path allocates, locks, performs I/O, or panics for documented inputs.
- [ ] Confirm the full architecture/OS/WASM Actions matrix is green.
- [ ] Commit any validation-only corrections as a final meaningful milestone and leave the working tree clean.

## Execution contract

- Keep the plan updated as work progresses and check off completed items.
- Commit each completed stage or meaningful milestone.
- Implementation steps may be revised when new evidence warrants it.
- Design rules may be revised for a documented, well-supported reason.
- Goals and acceptance criteria must not be changed without explicit user approval.
