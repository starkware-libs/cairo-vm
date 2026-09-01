# Onboarding

This document helps new developers navigate the Cairo VM codebase. It covers the project structure, configuration, production usage, and common pitfalls. For how the VM works conceptually, see [How does the Cairo VM work?](./vm/).

## Project structure

The repository is a Cargo workspace with these crates:

- `vm/` — Core library. Contains the VM, memory model, builtins, hint processors, types, and serialization. This is what downstream projects depend on.
- `cairo-vm-cli/` — CLI binary for running compiled Cairo 0 programs (JSON format).
- `cairo1-run/` — CLI binary for compiling and running Cairo 1 programs. Depends on the Cairo compiler crates.
- `cairo-vm-tracer/` — Library for the web-based execution tracer. Used by `cairo-vm-cli` when the `tracer` feature is enabled. See [tracer docs](./tracer/).
- `cairo1-hint-processor/` — `HintProcessor` for the hints the Cairo 1 compiler emits. The only crate besides `cairo1-run` that depends on `cairo-lang-*`.
- `hint_accountant/` — Dev tool that reports which hints from `cairo-lang` are implemented and which are missing.
- `examples/` — Usage examples: WASM demo, hyper-threading benchmarks, custom hint processor.
- `fuzzer/` — Fuzz testing and differential fuzzing against the Python VM.
- `ensure-no_std/` — CI-only crate that verifies the `vm` crate compiles without `std`.
- `bench/` — Criterion and iai-callgrind benchmark files, compiled as part of the `vm` crate (not a standalone crate).

### Inside `vm/src/`

The core crate has these top-level modules:

- `vm/vm_core.rs` — The VM itself: fetch-decode-execute loop, operand resolution, register updates.
- `vm/vm_memory/` — Memory model: write-once segments, relocation, validated memory.
- `vm/runners/` — `CairoRunner` (orchestrates a full run: init, execute, relocate, output) and builtin runners (pedersen, range_check, ecdsa, etc.).
- `vm/decoding/` — Instruction decoder: turns a 63-bit encoded felt into an `Instruction` struct.
- `vm/trace/` — Trace recording and relocation for the prover.
- `vm/context/` — `RunContext`: holds pc, ap, fp and computes operand addresses.
- `vm/security.rs` — Post-execution security checks (memory holes, builtin segment validation).
- `vm/hooks.rs` — Optional callbacks before/after each VM step.
- `types/` — Core data types: `Relocatable`, `MaybeRelocatable`, `Instruction`, `Program`, layouts, builtin definitions.
- `hint_processor/` — Hint execution: trait definition (`HintProcessor`), the built-in hint processor (implements all whitelisted hints), and the Cairo 1 hint processor.
- `serde/` — Deserialization of compiled program JSON and Program parsing.
- `air_public_input.rs` / `air_private_input.rs` — Serialization of AIR inputs for the prover (Stone/Stwo).
- `cairo_run.rs` — High-level `cairo_run` function that wires everything together.
- `math_utils/` — Field arithmetic helpers.

## Feature flags

The `vm` crate has several feature flags that control compilation:

- `std` (default) — Standard library support. Disable for `no_std`/WASM targets.
- `cairo-0-secp-hints` — Enables secp256k1/secp256r1 hint implementations for Cairo 0.
- `cairo-0-data-availability-hints` — Enables data availability-related hints for Cairo 0.
- `extensive_hints` — Allows extending the hint set at runtime from within a hint.
- `test_utils` — Exposes test utilities and derives `Arbitrary` for fuzzing.
- `tracer` — Marker flag used by `cairo-vm-cli`'s `with_tracer` feature to conditionally compile tracer support.

## Layouts

A layout defines which builtins are available and their parameters (ratio, instances per component, etc.). The VM rejects programs that use builtins not present in the selected layout.

Available layouts: `plain`, `small`, `dex`, `dex_with_bitwise`, `perpetual`, `starknet`, `starknet_with_keccak`, `recursive`, `recursive_large_output`, `recursive_with_poseidon`, `all_cairo`, `all_cairo_stwo`, `all_solidity`, `dynamic`.

- `plain`: No builtins. Only for programs that don't use any.
- `all_cairo`: All builtins enabled. Good default for development.
- `all_cairo_stwo`: Variant of `all_cairo` tailored for the Stwo prover.
- `dynamic`: Layout parameters are loaded from an external JSON file at runtime (`--cairo_layout_params_file`).
- The rest (`small`, `dex`, `recursive`, `starknet`, etc.) correspond to specific prover configurations used in production.

The layout must match what the prover expects. If you're generating traces for Stone or Stwo, use the layout the prover is configured for.

## Proof mode vs normal mode

**Normal mode:**
- Executes and gets output.
- Builtins must match layout (unless `--allow_missing_builtins`).
- Security checks enabled by default (`--secure_run`).
- Can generate Cairo PIE (`--cairo_pie_output`).

**Proof mode** (`--proof_mode`):
- Executes and generates a provable trace.
- Missing builtins always allowed.
- Security checks must be explicitly enabled.
- Can generate AIR inputs (`--air_public_input`, `--air_private_input`).

For prover integration you need proof mode with trace and memory files:
```
cairo-vm-cli program.json --layout all_cairo --proof_mode \
  --trace_file trace.bin --memory_file memory.bin \
  --air_public_input public_input.json --air_private_input private_input.json
```

## Running tests

```bash
make deps    # Install Python dependencies (cairo-lang) + cargo-llvm-cov
make test    # Run the full test suite
```

For running specific subsets:
```bash
cargo test -p cairo-vm           # Only the vm crate
cargo test -p cairo-vm-cli       # Only the CLI
cargo test -p cairo1-run          # Only cairo1-run
```

See also: [debugging docs](./debugging.md) for comparing output against the Python VM.

## Common issues

**"Builtin(s) [...] not present in layout X"** — The program uses a builtin not available in the selected layout. Switch to `all_cairo` or the correct layout.

**`UnknownHint`** — A hint in the program is not implemented by the current `HintProcessor`. Use `hint_accountant` to check coverage. If the hint is from `cairo-lang`, it may need to be implemented. If it's custom, you need a [custom HintProcessor](./hint_processor/).

**`InconsistentMemory`** — Code or a hint is trying to write to a memory cell that was already written with a different value. Memory in Cairo is immutable once written.

**`SecurityError` / `InsufficientAllocatedCells`** — Post-run security checks found issues (memory holes in builtin segments, unexpected values). This usually means a program or hint is malformed, or the wrong layout was used.

## Further reading

- [How does the Cairo VM work?](./vm/) — Memory model, instruction set, hints, builtins
- [How does the original Python VM work?](./python_vm/) — Detailed code analysis of `cairo-lang`
- [Custom Hint Processor](./hint_processor/) — How to implement your own hint processor
- [References parsing](./references_parsing/) — How variable references are resolved in hints
- [Tracer](./tracer/) — Visual step-by-step debugger
- [Debugging](./debugging.md) — Comparing outputs against the Python VM
- [Cairo whitepaper](https://eprint.iacr.org/2021/1063.pdf) — The formal specification
