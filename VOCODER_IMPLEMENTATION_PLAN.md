# Vocoder effect implementation plan

Planning branch: `feature/vocoder-effect` in `tinyviolin`.

## Goals and scope

Add a basic realtime vocoder to `tinyviolin` and expose it through the showcase and ShoopDaLoop's embedded Tiny Synth/FX integration. The audio input is the modulator, the MIDI-generated mono synth is the carrier, and the processed result precedes the existing post-effects.

In scope:

- A dependency-free 16-band analysis/synthesis filter-bank vocoder.
- Vocoder enable, dry/wet mix, and sensitivity controls.
- Public `AudioProcessor` settings/setters, reset behavior, validation, state persistence, documentation, and tests.
- Matching controls in the `tinyviolin-showcase` plugin/editor.
- ShoopDaLoop native and browser/engine integration, including state, UI, protocol propagation, and MIDI learn for continuous vocoder controls.

Out of scope:

- Host audio/MIDI routing changes.
- FFT processing, pitch tracking, formant shifting, selectable band counts, user-adjustable attack/release, or an unvoiced/sibilance noise path.
- MIDI learn inside `tinyviolin`; ShoopDaLoop remains responsible for learned CC mappings.
- MIDI control of boolean effect toggles, consistent with ShoopDaLoop's existing continuous-control-only MIDI learn model.

## Immutable acceptance criteria

1. With the vocoder disabled, `AudioProcessor` retains the current input-plus-synth signal path and existing effects behavior.
2. The enabled vocoder uses each audio input channel as an independent modulator for the shared mono synth carrier. At full wet, raw input and raw carrier are absent: no carrier produces silence, and no modulator produces silence after the fixed envelope release.
3. The basic algorithm uses 16 frequency bands with fixed setup-time coefficients and per-band envelope followers. It adds no block/FFT latency, produces finite bounded output at every accepted sample rate, and performs no allocation, locking, I/O, or logging during dispatch or processing.
4. Vocoder mix and sensitivity are finite normalized controls in `0.0..=1.0`. Mix interpolates from the existing dry input-plus-synth signal at `0.0` to vocoded output at `1.0`; sensitivity monotonically increases the response to a fixed non-clipping modulator signal. The vocoder is bypassed by default.
5. Signal order is vocoder/dry mixing, distortion, three-band EQ, compression, then reverb. Existing effects process the selected vocoder/dry result rather than independently processing or leaking the raw sources.
6. Enabling or bypassing the vocoder clears its analysis, carrier-filter, and envelope state. `reset_dsp()`/`panic()` clears that state while preserving vocoder settings.
7. `AudioProcessor` state round-trips vocoder settings, rejects invalid settings transactionally, and loads supported older state versions with the vocoder bypassed and default mix/sensitivity values.
8. The public core API, README, showcase parameters, and showcase editor consistently call the effect **Vocoder**, expose enable/mix/sensitivity, and do not call it a talkbox.
9. ShoopDaLoop exposes authoritative vocoder enable, mix, and sensitivity state in both native and browser-compatible paths. UI changes and restored processor state reach realtime processing, and current-state/session round-trips preserve the settings.
10. ShoopDaLoop MIDI learn lists vocoder mix and vocoder sensitivity as continuous targets. A learned channel/controller maps CC `0..=127` linearly across each normalized range, applies at the MIDI event's sample offset, updates frontend state, works while the vocoder is disabled, preserves the original MIDI event path, and survives session save/load under the existing assignment rules.
11. Existing synthesis, effects, MIDI, state compatibility, native/backend parity, and realtime allocation tests continue to pass.

## Design rules and constraints

- Preserve `tinyviolin`'s safe, dependency-free core and its Rust 1.85 minimum.
- Keep modulator and carrier separate until vocoder processing. Do not attempt to recover them after the current `input + synthesized` mix.
- Use a fixed-size, time-domain filter bank. Allocate or initialize all per-channel state in `AudioProcessor::new`; keep coefficient calculation and dynamic storage out of the callback.
- Place one vocoder state per configured audio channel, matching the existing independent multichannel effect model.
- Derive logarithmically spaced speech-band center frequencies and clamp them safely below Nyquist so all sample rates accepted by the library remain valid.
- Use fixed attack/release envelope timing and deterministic gain normalization. Tune constants with objective DSP tests plus a rendered speech/carrier fixture; do not add controls outside the accepted scope to compensate for tuning.
- Bypass stateful DSP without changing the existing dry calculation. Reset vocoder state on enable transitions using the same policy as the existing stateful effects.
- Extend serialized formats by versioning and retain explicit readers for currently supported older versions; loading remains validate-first and transactional.
- Keep stable plugin parameter IDs once introduced: `vocoder-enabled`, `vocoder-mix`, and `vocoder-sensitivity`.
- Keep ShoopDaLoop's CC assignment storage fixed-size and deterministic. Expand its canonical continuous-parameter enum/arrays rather than introducing a separate vocoder MIDI mapping path.
- Process learned CCs in ShoopDaLoop before forwarding the MIDI event to `tinyviolin`, as existing learned controls do. Do not broaden `tinyviolin`'s MIDI parser for host-owned MIDI learn.
- Implement ShoopDaLoop integration in a separate clean feature branch/worktree based on its intended target branch; do not modify the currently dirty `../shoopdaloop-akai` worktree.
- During coordinated development, use a reproducible `tinyviolin` dependency source in ShoopDaLoop and do not commit a machine-local path override.

## Staged implementation

### Stage 1 — Define and test the core vocoder DSP

- [x] Add fixed-size band-pass filter and envelope-follower primitives, setup-time coefficient generation, reset support, and a 16-band per-channel vocoder state in `src/effects.rs` (or a focused internal DSP module if that keeps responsibilities clearer).
- [x] Implement modulator analysis, carrier-band shaping, deterministic band summation/normalization, sensitivity scaling, denormal suppression, and finite output handling.
- [x] Add focused tests for band selectivity, envelope attack/release, sensitivity response, missing modulator/carrier behavior, reset behavior, low/common/high accepted sample rates, and long-run finite output.
- [x] Render or otherwise inspect a representative speech-like modulator with a harmonically rich carrier; tune only fixed coefficients/constants required for intelligible basic behavior.
- [x] Run core formatting, clippy, and focused tests.
- [x] Commit the core DSP milestone.

### Stage 2 — Integrate vocoder routing and public `AudioProcessor` controls

Depends on Stage 1.

- [x] Extend `EffectSettings` with bypassed-by-default vocoder enable plus normalized mix and sensitivity defaults; add validation errors and dedicated `AudioProcessor` setters.
- [x] Change the render path to pass separate input/modulator and synthesized carrier samples into per-channel processing, interpolate dry and vocoded paths, and then run the existing post-effect chain in the accepted order.
- [x] Reset vocoder state on enable transitions and from `reset_dsp()`/`panic()` without disturbing settings or unrelated effect state.
- [x] Add routing tests proving disabled compatibility, dry/full-wet behavior, per-channel modulation with a shared carrier, no raw-source leakage at full wet, sample-timed MIDI operation, and downstream effect processing.
- [x] Extend realtime-allocation coverage with the vocoder enabled.
- [x] Run the core test suite and commit the routing/API milestone.

### Stage 3 — Version core state and update public documentation/showcase

Depends on Stage 2.

- [ ] Bump the audio-state format, serialize vocoder controls, load current state transactionally, and provide bypassed defaults when reading supported older versions.
- [ ] Extend state tests for round-trip, malformed/out-of-range values, unchanged-on-error behavior, and all supported legacy versions.
- [ ] Document vocoder terminology, carrier requirements, controls, signal order, full-wet behavior, multichannel behavior, and realtime guarantees in `README.md` and public API docs.
- [ ] Add showcase host parameters and egui controls using the stable vocoder IDs; propagate settings into `ShowcaseProcessor` and include the vocoder in showcase allocation/default/UI tests.
- [ ] Update plugin feature metadata only if the host API has an appropriate vocoder category; do not substitute an inaccurate category.
- [ ] Run all `tinyviolin` workspace tests, docs, formatting, clippy, all-feature showcase checks, and dry-run packaging.
- [ ] Commit the state/documentation/showcase milestone.

### Stage 4 — Extend ShoopDaLoop's engine control and MIDI model

Depends on a consumable core revision from Stage 3.

- [ ] Create a clean ShoopDaLoop feature branch/worktree and update its pinned `tinyviolin` dependency to the selected feature release/revision.
- [ ] Extend Tiny Synth/FX control/editor state with vocoder enable, mix, and sensitivity setters/fields, and propagate them into `tinyviolin::AudioProcessor` on control-state synchronization and processor preparation.
- [ ] Add vocoder mix and sensitivity to the canonical continuous-parameter enum, fixed assignment storage, atomic runtime-value mirror, range conversion, current-state capture, and deterministic iteration. Resize fixed arrays from the parameter count rather than duplicating numeric lengths.
- [ ] Apply learned vocoder CC values at event offsets, including while bypassed, and publish resulting values back to authoritative editor state.
- [ ] Extend engine tests for direct setters, prepared/replaced processors, CC endpoints/intermediate values, exact channel/controller matching, disabled-effect control, state restoration, sample timing, and no allocation.
- [ ] Commit the ShoopDaLoop engine milestone.

### Stage 5 — Propagate ShoopDaLoop controls through native/browser APIs and persistence

Depends on Stage 4.

- [ ] Add exhaustive vocoder control/state/parameter variants and conversions through `shoop_app_api`, `shoop_backend`, native FX-chain dispatch, `shoop_audio_protocol`, `shoop_audio_worklet`, and `shoop_worklet_client`.
- [ ] Update native and browser-compatible snapshots so vocoder values and MIDI assignments remain authoritative after UI, CC, restore, processor replacement, and sample-rate changes.
- [ ] Extend session transfer and compatibility fixtures so vocoder assignments persist under existing Tiny Synth/FX assignment rules and older sessions default to no new assignments with the vocoder bypassed.
- [ ] Add focused native, worklet protocol, client conversion, browser-engine, remote-application, and session round-trip tests for every new exhaustive path.
- [ ] Run ShoopDaLoop formatting, warning-denying builds, changed-test policy checks, and targeted native/WASM tests.
- [ ] Commit the backend/protocol/persistence milestone.

### Stage 6 — Add ShoopDaLoop editor and MIDI-learn controls

Depends on Stage 5.

- [ ] Add a compact Vocoder enable control with mix and sensitivity sliders to the Tiny Synth/FX editor, driven by backend snapshots rather than optimistic persistent UI state.
- [ ] Add vocoder mix and sensitivity labels to the existing MIDI Learn target selector and assignment list; retain continuous-only behavior for learnable targets.
- [ ] Extend editor interaction tests for enabling, both sliders, disabled-slider presentation, target selection, assignment/removal/clear, deterministic listing, and isolation between track-scoped editor windows.
- [ ] Verify UI labels consistently use **Vocoder** and remain usable at the editor's supported sizes.
- [ ] Commit the ShoopDaLoop UI/MIDI-learn milestone.

### Stage 7 — End-to-end validation

Depends on all prior stages.

- [ ] In `tinyviolin`, run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`, `cargo test --workspace --doc`, `cargo doc --workspace --no-deps`, `cargo check -p tinyviolin-showcase --all-features --all-targets`, and `cargo publish -p tinyviolin --dry-run`.
- [ ] Exercise a direct core scenario with live-style modulator input, a rich MIDI carrier, sensitivity endpoint/intermediate changes, mix endpoint changes, reset, and state reload; confirm intelligible modulation, finite bounded output, and no raw-source leakage at full wet.
- [ ] In ShoopDaLoop, exercise native and browser-compatible scenarios: enable the vocoder, feed audio plus MIDI notes, learn a CC for sensitivity and mix, test endpoint/intermediate values and bypassed updates, verify frontend reflection, remove assignments, and save/reload the session.
- [ ] Run ShoopDaLoop's mandated formatting, warning, complete nextest, tracing-inventory, and WASM build/smoke gates from its repository instructions.
- [ ] Confirm both repositories contain only intended source, tests, docs, lockfile/dependency updates, and plan progress; remove generated audio/build artifacts.
- [ ] Commit final validation/documentation milestones in each repository.

## Execution contract

- Keep the plan updated as work progresses and check off completed items.
- Commit each completed stage or meaningful milestone.
- Implementation steps may be revised when new evidence warrants it.
- Design rules may be revised for a documented, well-supported reason.
- Goals and acceptance criteria must not be changed without explicit user approval.
