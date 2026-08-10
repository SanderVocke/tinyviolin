# Validation record

## Objective and completion criteria

The deliverable is a complete `tinyviolin` Rust crate implementing `PLAN.md`: ten generated instruments, block/sample-rate-aware event synthesis, bounded polyphony with callback-safe processing, fixed-size MIDI control and mappings, documentation/rendering, tests, and architecture/wasm CI. Development and validation must never send audio to a system audio device.

## Prompt-to-artifact checklist

| Requirement | Concrete artifact and evidence |
| --- | --- |
| Crate named `tinyviolin` | `Cargo.toml` package name; stable Cargo metadata and builds succeed. |
| Bass, pad, lead, sine, square, triangle, bass drum, tom, snare, hat | Public `Instrument` variants in `src/instrument.rs`; implementations in `src/dsp.rs`; all-preset bounded/finite tests. |
| Mathematically simple generated category sounds | Phase accumulators, elementary waveform functions, xorshift noise, simple envelopes, and drum pitch drops in `src/dsp.rs`; no samples and no dependencies. |
| Real-time block processing aware of sample rate/buffer/events | `Synth::new`, `Synth::dispatch`, and `Synth::process` in `src/engine.rs`; exact-offset and varying-block tests in `tests/core.rs`. |
| Polyphony, capped if necessary | Const-generic fixed voice array and documented deterministic stealing in `src/engine.rs`; exhaustion and stealing tests. |
| No allocations/locks on real-time thread | Fixed arrays throughout `src`; `tests/realtime_alloc.rs` counts zero allocations across prepared core and MIDI processing; source scan finds no allocation, lock, I/O, or logging types in `src`. |
| Controlled by events | `Event`, `TimedEvent`, `VoiceId`, and validation in `src/event.rs`; timing, overlap, release, all-notes-off, and malformed-stream tests. |
| Wrapper for at-most-four-byte MIDI messages | `MidiMessage` uses `[u8; 4]` plus a length in `src/midi.rs`; parser and integration tests cover supported/malformed statuses and lengths. |
| MIDI notes/channels map to instrument(s) and settings such as frequency | `MidiSynth` direct 16×128 table, fixed mapping layers, `MidiPitch::Note`/`Fixed`, gain, per-key and per-channel setup methods; layering/remapping tests in `tests/midi.rs`. |
| Keep it simple | No runtime dependencies (`cargo tree` contains only `tinyviolin`), safe library Rust, mono output, fixed presets/capacities, no backend. |
| GitHub Actions for major architectures and wasm | `.github/workflows/ci.yml` has stable quality gates and exact build entries for x86_64, i686, AArch64, ARMv7, and `wasm32-unknown-unknown` across Linux/Windows/macOS targets. `actionlint` accepts the workflow, and all ten target commands build locally on stable. |
| Usage and generated-output validation | `README.md` is included as crate rustdoc; three doctests pass. `examples/render_wav.rs` writes ten preset sections without opening an audio device. Programmatic WAV inspection confirms mono PCM16, 48 kHz, 480,000 frames/10 seconds, bounded samples, and non-silent signal in every section. |
| Never play system audio | No audio backend dependency or playback code exists. The render example only writes a file. Manual system-audio audition remains explicitly skipped in `PLAN.md`. |

## Immutable acceptance-criteria audit

1. **Ten documented instruments:** covered by `Instrument`, README table, and DSP tests.
2. **Generated elementary DSP only:** covered by `src/dsp.rs` and the empty dependency graph.
3. **Caller-owned mono block/sample timing:** covered by engine API docs and core timing/block tests.
4. **Fixed polyphony and deterministic stealing:** covered by fixed `[Voice; VOICES]` and stealing tests.
5. **Zero allocation/locking in processing:** covered by counting-allocator test, safe fixed-storage implementation, and source scan.
6. **Core event lifecycle and deterministic errors:** covered by event types and invalid frequency/gain/timing tests that assert no mutation.
7. **Length-tagged four-byte MIDI storage and validation:** covered by `MidiMessage`, parser tests, and allocation test.
8. **16×128 layered mapping with note/fixed pitch and reliable release:** covered by MIDI APIs and mapping/remapping/channel tests.
9. **Required automated DSP/engine/MIDI properties:** covered by unit and integration suites in `src/*` and `tests/*`.
10. **Runnable WAV example and integration guidance:** covered by `examples/render_wav.rs`, README, doctests, and file inspection.
11. **Exact CI architecture/wasm matrix:** covered by `.github/workflows/ci.yml` and successful local cross-builds for every named target.
12. **Stable local checks and green GitHub Actions:** stable local checks pass. GitHub Actions status is **unverified/blocking** because this local repository has no Git remote and therefore no Actions run to inspect.

## Commands verified locally

The final command set below was rerun from a detached clean worktree at commit `87a0b85`; it completed with `CLEAN CHECKOUT VALIDATION PASSED` and introduced only ignored build/render artifacts.

- `cargo +stable fmt --check`
- `cargo +stable clippy --all-targets -- -D warnings`
- `cargo +stable test --locked --all-targets`
- `cargo +stable test --locked --doc`
- `RUSTDOCFLAGS='-D warnings' cargo +stable doc --locked --no-deps`
- `cargo +stable test --release`
- `cargo +stable run --release --example render_wav`
- `cargo +stable build --locked --release --lib --target <target>` for every target listed in acceptance criterion 11
- `go run github.com/rhysd/actionlint/cmd/actionlint@latest .github/workflows/ci.yml`
- `cargo tree` and a source scan for allocation, synchronization, and I/O primitives

## Explicitly skipped or blocked

- **Skipped:** Listening to the rendered WAV through system audio, per the user's prohibition. No playback command or audio device API was used.
- **Blocked:** Observing a green GitHub Actions run. `git remote -v` returns no remote; a repository URL/push access or CI run evidence is required to close this gate.
