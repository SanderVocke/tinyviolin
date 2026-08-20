# Vocoder and noise gate effects implementation plan

Planning branch: `feature/vocoder-effect` in `tinyviolin`.

## Goals and scope

Add a basic realtime vocoder and a basic noise gate to `tinyviolin`, and expose both through the showcase and ShoopDaLoop's embedded Tiny Synth/FX integration. For the vocoder, audio input is the modulator and the MIDI-generated mono synth is the carrier. The gate normally suppresses only quiet audio input; when both effects are enabled, the same gate gain also suppresses the synth carrier so held MIDI sound remains quiet until microphone activity opens the gate.

In scope:

- A dependency-free 16-band analysis/synthesis filter-bank vocoder.
- Vocoder enable, dry/wet mix, and sensitivity controls.
- A dependency-free per-channel noise gate with enable and threshold controls, a fixed reasonable RMS tracking window, and fixed click-free opening/closing behavior.
- Intentional gate/vocoder interaction: with both enabled, microphone activity gates both the input/modulator and the MIDI-generated carrier.
- Public `AudioProcessor` settings/setters, reset behavior, validation, state persistence, documentation, and tests for both effects.
- Matching controls in the `tinyviolin-showcase` plugin/editor.
- ShoopDaLoop native and browser/engine integration, including state, UI, protocol propagation, and MIDI learn for vocoder mix/sensitivity and noise-gate threshold.

Out of scope:

- Host audio/MIDI routing changes.
- FFT processing, pitch tracking, formant shifting, selectable vocoder band counts, user-adjustable vocoder attack/release, or an unvoiced/sibilance noise path.
- Gate sidechains, lookahead, expansion ratios, hold/attack/release/hysteresis controls, or other advanced gate settings. Suitable fixed internal timing and hysteresis are allowed.
- MIDI learn inside `tinyviolin`; ShoopDaLoop remains responsible for learned CC mappings.
- MIDI control of boolean effect toggles, consistent with ShoopDaLoop's existing continuous-control-only MIDI learn model.

## Immutable acceptance criteria

1. With both new effects disabled, `AudioProcessor` retains the original input-plus-synth signal path and existing effects behavior.
2. The enabled vocoder uses each audio input channel as an independent modulator for the shared mono synth carrier. At full wet, raw input and raw carrier are absent: no carrier produces silence, and no modulator produces silence after the fixed envelope release.
3. The vocoder uses 16 frequency bands with fixed setup-time coefficients and per-band envelope followers. It adds no block/FFT latency, produces finite bounded output at every accepted sample rate, and performs no allocation, locking, I/O, or logging during dispatch or processing.
4. Vocoder mix and sensitivity are finite normalized controls in `0.0..=1.0`. Mix interpolates from the selected dry input-plus-synth signal at `0.0` to vocoded output at `1.0`; sensitivity monotonically increases the response to a fixed non-clipping modulator signal. The vocoder is bypassed by default.
5. The noise gate is bypassed by default and has one user-adjustable continuous threshold in `-80.0..=0.0 dB`, defaulting to `-50.0 dB`. It derives one gate gain per input channel from a trailing RMS window of approximately 10 ms, uses fixed internal hysteresis and click-free opening/closing timing, and reaches exact digital silence after its fixed closing period when the tracked level remains below threshold.
6. With the gate enabled and vocoder disabled, each channel's gate gain suppresses only that channel's audio input; MIDI-generated synth audio remains unaffected. Raising the threshold cannot make a fixed input open more readily.
7. With both gate and vocoder enabled, each channel's gate gain applies to both its input/modulator and its copy of the shared synth carrier before dry/vocoder mixing. A held MIDI note therefore produces no new output after the gate closes, including at vocoder mix `0.0`; microphone activity above threshold opens the gate and reveals the already-held carrier without requiring a new MIDI note. Existing downstream effect tails may decay according to their own fixed state.
8. Signal order is gate source selection, vocoder/dry mixing, distortion, three-band EQ, compression, then reverb. Existing post-effects process only the selected gate/vocoder/dry result rather than independently processing or leaking raw sources.
9. Enabling or bypassing either new effect clears that effect's tracking/filter/envelope state. `reset_dsp()`/`panic()` clears both effects' DSP state while preserving their settings.
10. `AudioProcessor` state round-trips all vocoder and noise-gate settings, rejects invalid settings transactionally, and loads every supported older state version with newly absent effects bypassed and their documented default control values.
11. The public core API, README, showcase parameters, and showcase editor consistently call the effects **Vocoder** and **Noise Gate**. They expose vocoder enable/mix/sensitivity and gate enable/threshold; vocoder documentation does not call it a talkbox.
12. ShoopDaLoop exposes authoritative enable and continuous-control state for both effects in native and browser-compatible paths. UI changes and restored processor state reach realtime processing, and current-state/session round-trips preserve all settings.
13. ShoopDaLoop MIDI learn lists vocoder mix, vocoder sensitivity, and noise-gate threshold as continuous targets. A learned channel/controller maps CC `0..=127` linearly across the target range, applies at the MIDI event's sample offset, updates frontend state, works while its effect is disabled, preserves the original MIDI event path, and survives session save/load under existing assignment rules. Boolean enables remain non-learnable.
14. Existing synthesis, effects, MIDI, state compatibility, native/backend parity, and realtime-allocation tests continue to pass.

## Design rules and constraints

- Preserve `tinyviolin`'s safe, dependency-free core and its Rust 1.85 minimum.
- Keep modulator and carrier separate until vocoder processing. Do not attempt to recover them after the current `input + synthesized` mix.
- Use a fixed-size, time-domain filter bank. Allocate or initialize all per-channel state in `AudioProcessor::new`; keep coefficient calculation and dynamic storage out of the callback.
- Place one vocoder state and one noise-gate tracker per configured audio channel, matching the existing independent multichannel effect model.
- Derive logarithmically spaced speech-band center frequencies and clamp them safely below Nyquist so all sample rates accepted by the library remain valid.
- Use fixed attack/release envelope timing and deterministic gain normalization. Tune constants with objective DSP tests plus a rendered speech/carrier fixture; do not add controls outside the accepted scope to compensate for tuning.
- Compute gate level from raw input before vocoder analysis. Use a preallocated trailing RMS window of approximately 10 ms plus fixed internal hysteresis and gain smoothing; add no lookahead or user-adjustable timing controls.
- When the gate is enabled, gate the raw input before constructing the dry path or feeding the vocoder. Gate the carrier only when the vocoder is also enabled, and do so before both dry and wet paths so mix `0.0` cannot leak a held carrier through a closed gate.
- Bypass stateful DSP without changing the applicable dry calculation. Reset each effect's internal state on its enable transitions using the same policy as existing stateful effects.
- Extend serialized formats by versioning and retain explicit readers for currently supported older versions; loading remains validate-first and transactional. The gate addition follows the implemented vocoder state version rather than rewriting it.
- Keep stable plugin parameter IDs once introduced: `vocoder-enabled`, `vocoder-mix`, `vocoder-sensitivity`, `noise-gate-enabled`, and `noise-gate-threshold`.
- Keep ShoopDaLoop's CC assignment storage fixed-size and deterministic. Expand its canonical continuous-parameter enum/arrays rather than introducing effect-specific MIDI mapping paths.
- Process learned CCs in ShoopDaLoop before forwarding the MIDI event to `tinyviolin`, as existing learned controls do. Do not broaden `tinyviolin`'s MIDI parser for host-owned MIDI learn.
- Implement ShoopDaLoop integration in a separate clean feature branch/worktree based on its intended target branch; do not modify the currently dirty `../shoopdaloop-akai` worktree.
- During coordinated development, use a reproducible `tinyviolin` dependency source in ShoopDaLoop and do not commit a machine-local path override.

## Scope-expansion audit

- The user explicitly expanded the immutable scope after the vocoder implementation was underway; this revision preserves completed work and adds the noise gate as new unchecked work.
- `tinyviolin` vocoder Stages 1–3 are implemented and committed on `feature/vocoder-effect`; the branch was already pushed before this scope expansion.
- ShoopDaLoop vocoder Stages 4–6 are implemented and committed on `feature/tiny-synth-vocoder` in the clean `../shoopdaloop-vocoder` worktree. Those commits are not yet pushed, and no PR has been opened.
- No noise-gate implementation is claimed by the checked vocoder stages. All core, state, showcase, ShoopDaLoop, interaction, and final-validation work introduced for the gate appears below as unchecked tasks.
- Final end-to-end evidence was incomplete at expansion time: targeted and warning-denying checks had passed, but the complete ShoopDaLoop nextest/browser gate and final prompt-to-artifact audit had not completed. They remain required in the final stage.

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

- [x] Bump the audio-state format, serialize vocoder controls, load current state transactionally, and provide bypassed defaults when reading supported older versions.
- [x] Extend state tests for round-trip, malformed/out-of-range values, unchanged-on-error behavior, and all supported legacy versions.
- [x] Document vocoder terminology, carrier requirements, controls, signal order, full-wet behavior, multichannel behavior, and realtime guarantees in `README.md` and public API docs.
- [x] Add showcase host parameters and egui controls using the stable vocoder IDs; propagate settings into `ShowcaseProcessor` and include the vocoder in showcase allocation/default/UI tests.
- [x] Update plugin feature metadata only if the host API has an appropriate vocoder category; do not substitute an inaccurate category.
- [x] Run all `tinyviolin` workspace tests, docs, formatting, clippy, all-feature showcase checks, and dry-run packaging.
- [x] Commit the state/documentation/showcase milestone.

### Stage 4 — Extend ShoopDaLoop's engine control and MIDI model

Depends on a consumable core revision from Stage 3.

- [x] Create a clean ShoopDaLoop feature branch/worktree and update its pinned `tinyviolin` dependency to the selected feature release/revision.
- [x] Extend Tiny Synth/FX control/editor state with vocoder enable, mix, and sensitivity setters/fields, and propagate them into `tinyviolin::AudioProcessor` on control-state synchronization and processor preparation.
- [x] Add vocoder mix and sensitivity to the canonical continuous-parameter enum, fixed assignment storage, atomic runtime-value mirror, range conversion, current-state capture, and deterministic iteration. Resize fixed arrays from the parameter count rather than duplicating numeric lengths.
- [x] Apply learned vocoder CC values at event offsets, including while bypassed, and publish resulting values back to authoritative editor state.
- [x] Extend engine tests for direct setters, prepared/replaced processors, CC endpoints/intermediate values, exact channel/controller matching, disabled-effect control, state restoration, sample timing, and no allocation.
- [x] Commit the ShoopDaLoop engine milestone.

### Stage 5 — Propagate ShoopDaLoop controls through native/browser APIs and persistence

Depends on Stage 4.

- [x] Add exhaustive vocoder control/state/parameter variants and conversions through `shoop_app_api`, `shoop_backend`, native FX-chain dispatch, `shoop_audio_protocol`, `shoop_audio_worklet`, and `shoop_worklet_client`.
- [x] Update native and browser-compatible snapshots so vocoder values and MIDI assignments remain authoritative after UI, CC, restore, processor replacement, and sample-rate changes.
- [x] Extend session transfer and compatibility fixtures so vocoder assignments persist under existing Tiny Synth/FX assignment rules and older sessions default to no new assignments with the vocoder bypassed.
- [x] Add focused native, worklet protocol, client conversion, browser-engine, remote-application, and session round-trip tests for every new exhaustive path.
- [x] Run ShoopDaLoop formatting, warning-denying builds, changed-test policy checks, and targeted native/WASM tests.
- [x] Commit the backend/protocol/persistence milestone.

### Stage 6 — Add ShoopDaLoop editor and MIDI-learn controls

Depends on Stage 5.

- [x] Add a compact Vocoder enable control with mix and sensitivity sliders to the Tiny Synth/FX editor, driven by backend snapshots rather than optimistic persistent UI state.
- [x] Add vocoder mix and sensitivity labels to the existing MIDI Learn target selector and assignment list; retain continuous-only behavior for learnable targets.
- [x] Extend editor interaction tests for enabling, both sliders, disabled-slider presentation, target selection, assignment/removal/clear, deterministic listing, and isolation between track-scoped editor windows.
- [x] Verify UI labels consistently use **Vocoder** and remain usable at the editor's supported sizes.
- [x] Commit the ShoopDaLoop UI/MIDI-learn milestone.

### Stage 7 — Add the core noise-gate tracker and routing

Depends on the completed vocoder core and routing.

- [x] Add a per-channel, fixed-window RMS tracker and gate-gain state with threshold conversion, fixed hysteresis, fixed click-free opening/closing behavior, denormal suppression, finite-output handling, and reset support. Preallocate all tracking storage in processor construction.
- [x] Extend `EffectSettings` with bypassed-by-default noise-gate enable and `-50.0 dB` threshold; validate `-80.0..=0.0 dB` transactionally and add dedicated `AudioProcessor` setters.
- [x] Apply gate gain to raw input before dry/vocoder routing. Apply the same gain to the carrier only when both gate and vocoder are enabled, before both sides of the vocoder mix.
- [x] Add focused DSP tests for the approximately 10 ms RMS window, threshold endpoints/intermediate values, fixed hysteresis, opening/closing timing, exact steady-state silence, reset, low/common/high sample rates, denormal handling, and long-run finite bounded output.
- [x] Add the four-way interaction matrix: both disabled compatibility; gate-only input muting with unaffected synth; vocoder-only compatibility; and combined operation where a held carrier closes and reopens from microphone level without another MIDI event. Cover mix `0.0`, intermediate mix, full wet, sensitivity changes, multichannel independence, post-effect ordering, and no raw carrier leakage.
- [x] Extend realtime-allocation tests with gate-only and combined gate/vocoder processing, including MIDI dispatch while the gate opens and closes.
- [x] Run core formatting, warning-denying clippy, focused tests, and allocation tests; inspect objective envelope/gain traces to tune only fixed internal constants.
- [x] Commit the core noise-gate DSP/routing milestone.

### Stage 8 — Version gate state and update core documentation/showcase

Depends on Stage 7.

- [x] Bump the audio-state format after the vocoder format, serialize gate enable/threshold, load current state transactionally, and load supported v1/v2/v3 state with the gate bypassed and threshold at `-50.0 dB`.
- [x] Extend state tests for gate round-trip, malformed/non-finite/out-of-range threshold, unchanged-on-error behavior, and explicit defaults for every supported legacy version.
- [x] Document gate threshold units/range/default, fixed tracking behavior, multichannel behavior, signal order, and the combined gate/vocoder carrier rule in `README.md` and public API docs.
- [x] Add `noise-gate-enabled` and `noise-gate-threshold` showcase parameters and compact **Noise Gate** editor controls. Display threshold in dB and propagate it through `ShowcaseProcessor`.
- [x] Extend showcase parameter/default/UI and realtime-allocation tests with gate-only and combined gate/vocoder cases.
- [x] Run all `tinyviolin` workspace tests, docs, formatting, warning-denying clippy, all-feature showcase checks, and dry-run packaging.
- [x] Commit the gate state/documentation/showcase milestone and push a consumable `tinyviolin` revision for ShoopDaLoop.

### Stage 9 — Extend ShoopDaLoop engine control and MIDI learn for the gate

Depends on a consumable Stage 8 core revision.

- [x] Update ShoopDaLoop's pinned `tinyviolin` revision without using a local path override.
- [x] Extend Tiny Synth/FX control/editor state with noise-gate enable and threshold, and synchronize both into prepared, restored, replaced, and sample-rate-reconfigured processors.
- [x] Add noise-gate threshold to the canonical continuous-parameter enum, fixed assignment storage, atomic runtime mirror, `-80.0..=0.0 dB` CC conversion, current-state capture, labels, and deterministic iteration. Derive all fixed array lengths from the canonical parameter count.
- [x] Apply learned threshold CC values at exact event offsets while preserving MIDI forwarding and authoritative frontend updates, including while the gate is disabled.
- [x] Extend engine tests for direct gate setters, CC endpoints/intermediate values, exact channel/controller matching, disabled-gate updates, assignment conflict/removal/clear, state restoration, processor replacement, sample timing, the combined held-note gate/vocoder scenario, and no allocation.
- [x] Run ShoopDaLoop formatting, warning-denying engine builds, changed-test policy, focused engine tests, and allocation tests.
- [x] Commit the ShoopDaLoop gate engine/MIDI milestone.

### Stage 10 — Propagate gate controls through ShoopDaLoop backends, persistence, and UI

Depends on Stage 9.

- [x] Add exhaustive noise-gate control/state/parameter variants and conversions through `shoop_app_api`, `shoop_backend`, native FX-chain dispatch, `shoop_audio_protocol`, `shoop_audio_worklet`, and `shoop_worklet_client`.
- [x] Keep native and browser-compatible snapshots authoritative after UI changes, learned CCs, restore, processor replacement, and sample-rate changes.
- [x] Extend session transfer, format documentation, and compatibility fixtures so threshold assignments persist and older sessions default to no gate assignment, gate bypassed, and threshold `-50.0 dB`.
- [x] Add a compact **Noise Gate** enable control and dB threshold slider to the Tiny Synth/FX editor. Add threshold to MIDI Learn target/assignment UI without making the boolean enable learnable.
- [x] Extend native, protocol, worklet, client, browser-engine, remote-application, session, and editor tests. UI tests must cover disabled-slider presentation, threshold interaction, learn/remove/clear, deterministic listing, supported editor sizes, and isolation between track-scoped windows.
- [x] Exercise native and browser-compatible combined behavior with a held MIDI note: closed mic gate yields silence after downstream tails, mic activity opens the already-held carrier, and closing the mic gate suppresses it again at dry, intermediate, and full-wet vocoder mixes.
- [x] Run ShoopDaLoop formatting, warning-denying workspace builds, changed-test policy, targeted native/WASM tests, and tracing inventory.
- [x] Commit the backend/protocol/persistence/UI milestone.

### Stage 11 — End-to-end validation and completion audit

Depends on all prior stages.

- [x] In `tinyviolin`, run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`, `cargo test --workspace --doc`, `cargo doc --workspace --no-deps`, `cargo check -p tinyviolin-showcase --all-features --all-targets`, and `cargo publish -p tinyviolin --dry-run`.
- [x] Exercise a direct core scenario with live-style quiet/loud microphone input and a rich held MIDI carrier. Cover gate threshold endpoints/intermediate values, vocoder sensitivity and mix endpoints/intermediate values, all four enable combinations, reset, panic, and state reload; confirm intelligible modulation, finite bounded output, exact closed-gate silence under the defined conditions, and no raw-source leakage.
- [x] In ShoopDaLoop, exercise native and browser-compatible scenarios for both effects: manipulate all controls, learn CCs for sensitivity/mix/threshold, test endpoint/intermediate and bypassed updates, verify frontend reflection and sample timing, remove assignments, and save/reload the session.
- [x] Run ShoopDaLoop's mandated formatting, warning-denying build, complete nextest, changed-test-policy, tracing-inventory, WASM build, and browser smoke/test gates from its repository instructions. Record toolchain substitutions separately; do not treat a proxy check as the mandated gate.
- [x] Audit every acceptance criterion against concrete tests, commands, source paths, and manual/proxy evidence. Separate confirmed behavior from any blocked or approximate evidence and do not declare completion while a criterion lacks defensible evidence.
- [x] Confirm both repositories contain only intended source, tests, docs, lockfile/dependency updates, and plan progress; remove generated audio/build artifacts.
- [ ] Commit and push final validation/documentation milestones on both feature branches and open/update the relevant PRs with dependency ordering and verification evidence.

## Completion audit and validation evidence

Objective deliverables are: implement every checked core/showcase/ShoopDaLoop task; preserve compatibility and realtime guarantees; commit and push both feature branches; open dependency-ordered PRs; and leave no unchecked plan item or unintended artifact.

Acceptance-criterion evidence:

1. Original bypass behavior: `audio::tests::synth_is_equal_on_every_channel_and_input_is_preserved` and the complete `tinyviolin` suite.
2. Independent full-wet modulation and source absence: `full_wet_vocoder_uses_input_channels_as_independent_modulators`, `full_wet_vocoder_does_not_leak_a_modulator_without_a_carrier`, and vocoder source/envelope tests in `src/effects.rs`.
3. Fixed 16-band, finite, callback-safe vocoder: constants/arrays in `src/effects.rs`, sample-rate/long-run tests, and `tests/realtime_alloc.rs`.
4. Vocoder controls/defaults/interpolation/sensitivity: `EffectSettings`, validation/setter tests, sensitivity-response tests, and zero-mix routing tests.
5. Gate threshold/tracking/silence: `NoiseGate` in `src/effects.rs` plus RMS, hysteresis, timing, monotonic-threshold, sample-rate, reset, and exact-silence tests.
6. Gate-only source semantics: `noise_gate_only_suppresses_quiet_input_without_suppressing_synth`.
7. Combined held-carrier semantics at dry/intermediate/full-wet mixes: core combined/per-channel tests, ShoopDaLoop engine held-note test, browser all-mixes test, and native dummy open/close/reopen test.
8. Signal order/no leakage: `ChannelEffects::process` and core routing/post-effect tests.
9. Toggle/reset/panic state: reset methods in `src/effects.rs`, vocoder/gate reset tests, toggle tests, and `audio_panic_clears_voices_and_effect_tails_but_not_configuration`.
10. Transactional state compatibility: audio state v4 implementation and v1/v2/v3/current round-trip/error tests.
11. Public terminology and controls: `README.md`, public API docs, stable showcase parameter IDs, showcase defaults/editor, and a repository search finding no production use of “talkbox”.
12. Native/browser authoritative ShoopDaLoop state: engine/backend/native/worklet/client/app propagation and native, browser, remote-application, processor-replacement, sample-rate-switch, and session tests.
13. Continuous-only MIDI learn: canonical ten-parameter arrays, gate/vocoder CC endpoint/intermediate/sample-offset/disabled-effect tests, UI assignment/remove/clear tests, protocol/worklet conversion tests, and session round-trips. Boolean enables are absent from the parameter enum.
14. Regression and realtime safety: all project gates below, including complete native and Wasm suites and allocation tests.

Final command evidence:

- `tinyviolin`: formatting, warning-denying workspace clippy, all-target workspace tests, doctests, docs, all-feature showcase check (with ALSA/JACK development libraries), allocation tests, and package dry-run passed.
- ShoopDaLoop formatting, changed-test policy, tracing inventory, warning-denying workspace build, compiler-only UI/worklet Wasm builds, and targeted native/browser/worklet/session/editor tests passed.
- Complete ShoopDaLoop nextest: 1,457 passed and 2 skipped with `SHOOP_ALLOW_MISSING_BACKENDS=1`; `ALSA_CONFIG_PATH=/dev/null` made unavailable host MIDI facilities deterministically skip rather than time out.
- Complete shared Wasm suites passed under both Node 22.22.2 and pinned Chrome/ChromeDriver 147.0.7727.117. Each ran 1,210 tests across 15 packages with no failure.
- Trunk 0.21.14 built the hosted browser application; Chrome hosted and self-contained output-only AudioWorklet smokes passed. Generated `dist`, worklet, and rendered-audio artifacts were removed from source worktrees; ordinary ignored Cargo `target` evidence remains untracked.
- Toolchain substitution: ShoopDaLoop's pinned stable Rust 1.94.1 currently refuses its already-resolved egui 0.36.1 packages, which declare Rust 1.95. The exact warning-build, nextest, Wasm-build, Node, and Chrome gates therefore ran with nightly Rust 1.97.0; the stable failure was reproduced and recorded rather than presented as a pass. This changes no source or lockfile dependency selection.
- Optional Firefox smoke was attempted with Nix Firefox 150.0.1/geckodriver 0.36.0 but did not pass its pre-audio startup assertion; Firefox was not an installed project browser, while the pinned Chrome browser gates above passed.

## Execution contract

- Keep the plan updated as work progresses, and check off an item only after its claimed code, test, or command evidence exists.
- Commit each completed stage or meaningful milestone.
- After each implementation attempt, inspect the focused failure/output and diff, choose the smallest defensible next change, rerun focused checks, and periodically rerun the relevant wider regression gate.
- Implementation steps may be revised when new evidence warrants it.
- Design rules may be revised for a documented, well-supported reason.
- Goals and acceptance criteria must not be changed without explicit user approval.
- If credentials, network access, toolchain/browser availability, an upstream dependency, or a product decision blocks completion and no defensible local path remains, stop with the exact unchecked requirement, evidence gathered, attempted paths, blocker, and input needed to continue; do not relabel proxy evidence as completion.
