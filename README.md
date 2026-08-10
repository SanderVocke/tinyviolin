# tinyviolin

`tinyviolin` is a small Rust library for generated mono instrument sounds. It
provides basic oscillators, melodic presets, percussion, fixed-capacity
polyphony, sample-timed control events, and a fixed-storage MIDI wrapper.

The API is designed for real-time callbacks: create the engine and mappings
before streaming, then pass caller-owned event and output slices to processing.
The processing paths do not allocate or lock.

Implementation is in progress according to [`PLAN.md`](PLAN.md).
