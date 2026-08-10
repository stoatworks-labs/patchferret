# AGENTS.md — PatchFerret

Onboarding for whoever (or whatever) picks this up next. `README.md` is the user-facing
description; this file is the *why*, the invariants, and an honest account of what is verified.

## What this is

A tool that reads a mixing console show file and emits documentation — patch list, spec sheet,
wiring topology — as PDFs. Runs in a browser (WASM, nothing uploaded) and as a CLI.

Started 2026-08-10. Public, MIT. Sibling private repo `patchferret-research` holds format
reverse-engineering notes and sample show files.

## The mental model

Everything hinges on one pipeline:

```
native show file → ShowAdapter → Show (PFX) → reports / XML
```

**The `Show` model is the only contract.** Reports never see a `.scn`. Adapters never see a PDF.
Adding a console is one new module implementing `ShowAdapter` plus one line in
`patchferret_formats::adapters()` — nothing downstream changes. Keep it that way; the value of
this repo is the model, not the X32 parser.

### Load-bearing invariants

Break these and the output becomes confidently wrong rather than obviously broken:

1. **The input-slot indirection must survive into the model.** Consoles put a re-patchable stage
   between the physical connector and the channel. `InputPatch` keeps `slot`, `socket` *and*
   `strip` precisely so the patch list can show the whole chain and so re-patching is
   expressible later. Do not "simplify" this by resolving connector→channel at parse time.
2. **Head amps belong to a `SocketRef`, not a `StripId`.** Preamp gain lives on the connector.
   On a shared stage box several consoles share it. Moving it onto the channel would silently
   misrepresent gain sharing.
3. **Anything not modelled becomes a `Diagnostic`.** Never drop a recognised element silently.
   The spec sheet's fidelity section and the CLI's `info` output are built from diagnostics, and
   they are the basis of the claim "this is what a conversion would lose".
4. **`SocketRef` indices are 1-based and match the silkscreen.** Adapters do the conversion from
   whatever internal numbering the vendor uses. `headamp_socket()` in the X32 adapter is the
   worked example: a flat 0–127 index splits into local 1–32, AES50-A 1–48, AES50-B 1–48.
5. **PFX XML is a published spec.** `xml.rs` is hand-written, not derived, so that renaming a
   Rust field cannot silently change the wire format. Round-trip tests enforce stability.

## Traps that have already cost time

- **The generic match arm shadows the specific ones.** In `x32.rs`, `[section, idx, "config"]`
  will swallow `["main", "st", "config"]` if it comes first, and Main/Mono strips vanish with no
  error. The specific arms are placed above it with a comment saying why. `rustc`'s
  `unreachable_patterns` warning is what caught it — do not silence that lint here.
- **PDF xref offsets are byte offsets into a Latin-1 file.** Building the document as a Rust
  `String` and converting at the end shifts every offset by the width of the non-ASCII header
  comment, producing a file some readers reject and others silently repair. `Document::finish`
  assembles a `Vec<u8>` for this reason, and a test walks the xref table checking each entry
  lands on `<n> 0 obj`.
- **WinAnsiEncoding is not Latin-1.** The em dash, ellipsis and curly quotes live at 0x80–0x9F,
  which Latin-1 leaves undefined. Without `winansi_high()` every em dash renders as `?` — which
  is exactly what the first patch list did, in every empty cell.
- **`Font::width` must know any character the reports actually emit**, or `Page::fit`
  under-measures and text overruns its column. Non-ASCII punctuation is enumerated explicitly.
- **The "not a scene file" guard must count recognised paths**, not resulting strips. A scene
  with only head amps is a valid parse; an early version rejected it.

## Environment

Zero runtime dependencies beyond `quick-xml` and `thiserror`. Deliberate:

- **The PDF writer is hand-rolled** (`patchferret-report/src/pdf.rs`) because the reports must
  build for `wasm32-unknown-unknown`. General PDF crates pull in image decoders, font shapers
  and filesystem access. The reports only need text, rules and boxes in the 14 standard fonts,
  which need no font embedding at all.
- **The WASM boundary is a raw C ABI**, not `wasm-bindgen`. The interface is bytes in, bytes
  out, so generated glue would do almost nothing, and avoiding it means the browser build needs
  no `wasm-pack`, no `wasm-bindgen-cli`, and no version handshake. `cargo build --target
  wasm32-unknown-unknown` is the entire toolchain.

```bash
cargo test                      # 65 tests
./scripts/build-web.sh          # browser bundle
python3 -m http.server 8731 --directory web
```

`web/patchferret.wasm` is a build artefact and is gitignored — the page will not work until
`build-web.sh` has run.

## Verified vs assumed

**Verified — asserted in CI against a real 2,104-line scene file** saved by an actual X32
(`tests/fixtures/x32-soundboard.scn`, from a public GitHub repository):

- Format detection, header, show name and version.
- All 32 input channels, 16 buses, 6 matrices, 8 DCAs, 8 aux ins read with names verbatim.
- Connector resolution through the real routing blocks, including the block-4 crossover from
  AES50-A to AES50-B.
- Head-amp retention only for connectors the show patches (32 of the console's 128).
- Every one of the 42 output rows decodes to a known source — no fallbacks taken.
- Full round trip through PFX XML with byte-stable re-serialisation.
- PDFs are generated, the xref table is walked, and all three were rendered and inspected.

**Verified by construction, not by hardware:**

- The X32 signal enumerations. See README "Provenance". The matrix-count check is a genuine
  falsification test, not a rationalisation, but neither enum has been confirmed on a desk.

**Assumed / unverified — do not claim otherwise:**

- That the generated documentation matches what the console's own screens show. Nobody has sat
  at an X32 with a printed patch list and compared them.
- Console profiles in `profile.rs` are transcribed from published specifications.
- The X-Air variants share the `.scn` dialect closely enough. The adapter claims them; only
  full-size X32 files have been tested.
- Everything about Yamaha, Allen & Heath, DiGiCo and Avid formats. No adapter exists.

**Explicitly not built:** writing show files, converting between consoles, and conforming to a
target profile. `ConsoleProfile` exists to make the third tractable but nothing consumes it yet.

## Conventions

Follows the fleet: `AGENTS.md` is the onboarding doc, the AI-assistance disclaimer sits at the
top of `README.md`, and the "verified vs assumed" split above is the part to keep honest as the
project grows. Sample show files are client data — they belong in `patchferret-research`, never
here. The one fixture in `tests/fixtures/` is from a public repository and is safe to ship.
