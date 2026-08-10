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
- **Yamaha field names differ per model, and getting it wrong fails silently.** DM3 has
  `Patch/Source` at 4 bytes; TF has `Patch/Select` at 1; DM7 has `InPatch` and puts `Signal`
  first. Hardcoding DM3's names made TF resolve zero connectors *and emit no diagnostic* — a
  silent wrong answer. Find the collection by any known name, then take whatever parameter it
  actually contains, and treat "found the collection but got no value" as a diagnostic.
- **A default patch is a sequential run, but not always an adjacent one.** Scanning for ascending
  byte runs found the Avantis and CL/QL tables instantly and finds nothing at all on SQ, whose
  values sit 336 bytes apart inside per-channel records. The heuristic failing is not evidence a
  format lacks a patch table.
- **A&H shows do not name the console model.** A numbered scene leads with its own *name*, and
  the factory FOH show happens to call scene 002 "Avantis" — which looks exactly like a model
  field and is not one. A detector built on that passes on the factory shows and fails on every
  user-saved one. The adapter says the model is unknown instead.
- **MMSXLIT mixes endianness with the container.** Container record headers are big-endian;
  the schema metadata and the packed values are little-endian. Reading the DM3 patch word
  big-endian also yields tidy ascending indices, so "it looks right" proves nothing.

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
cargo test                      # 146 tests
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

**Yamaha DM3 / DM7 / TF — verified against the 41 factory scenes** shipped inside DM3 Editor and
TF Editor (vendor content in a licensed install, so it cannot be committed; the integration test
skips when the editors are absent and the unit tests use synthetic containers):

- The `#YAMAHA MBDF…` container, parsed unchanged across two subtypes and two models.
- MMSXLIT is **self-describing** — schema records carry offset, datasize, arraysize, type and
  width — which is why the adapter needs no vendor descriptor files and works in WASM.
- The reconstructed schema tree sums exactly to the declared root size, and the walk yields
  exactly the collection/parameter counts the editor's `mms_Mixing.xml` declares.
- Channel names, colours and icons decode as real text.

**Allen & Heath Avantis / dLive — the patch table was located by controlled diff**, not
inference: store a show from Director offline, change exactly one patch point, store again,
compare. Nine bytes differed. Two things that generalise:

- **Compare decompressed contents, not files.** Every scene tarball differs between two saves on
  gzip's embedded timestamp alone, so `diff -rq` reports sixty changed files and tells you
  nothing.
- **The patch is not scene-recallable** — it lives only in `StageBoxScene65535`, the live state.
  Reading it from "the current scene" yields nothing.

**Yamaha CL/QL and A&H SQ — patch tables located by controlled diff** in the manufacturers' own
offline editors, and the adapters are checked against those exact files (gated behind
`PF_CLF_BASE` / `PF_SQ_NVDATA` etc., skipping when absent). Two things to keep honest:

- **CL/QL's patch table is at an ABSOLUTE offset** (`0x00d74b`) established on one QL5 written by
  one editor build. Nothing in the file points at it. The adapter therefore checks that what it
  finds there actually decodes, and reports "probably not the table" rather than printing 64
  channels of noise if another frame size moves it.
- **SQ's patch byte is a socket NUMBER with no class.** Only a Local patch was ever observed; an
  SQ can also take SLink, USB and I/O Port. The device is deliberately labelled "Input socket
  (class not decoded)" rather than "Local", because naming it would read as a fact.

**Verified by construction, not by hardware:**

- The X32 signal enumerations. See README "Provenance". The matrix-count check is a genuine
  falsification test, not a rationalisation, but neither enum has been confirmed on a desk.
- The Yamaha **patch-source encoding**. DM3's word splits into an index plus a type code
  (`0x0140`, `0x0160`), and the indices run sequentially across every factory scene — consistent
  with the analog inputs, but never checked against a console's patch screen. On **TF nothing
  resolves**: its field is one byte and names a selector, not a port. Both cases emit a
  diagnostic instead of a connector.

**Assumed / unverified — do not claim otherwise:**

- That the generated documentation matches what the console's own screens show. Nobody has sat
  at an X32 with a printed patch list and compared them.
- Console profiles in `profile.rs` are transcribed from published specifications.
- The X-Air variants share the `.scn` dialect closely enough. The adapter claims them; only
  full-size X32 files have been tested.
- Everything about Yamaha, Allen & Heath, DiGiCo and Avid formats. No adapter exists.

**Explicitly not built:** writing show files, converting between consoles, and conforming to a
target profile. `ConsoleProfile` exists to make the third tractable but nothing consumes it yet.

## Job metadata and the header

`JobInfo` is **deliberately not part of `Show`**. `Show` means "what the file says"; the event
name and the engineer's phone number are user-typed, and mixing them in would destroy the one
property that makes the model trustworthy. Reports take both.

Both front ends feed it the *same* `key: value` sheet — the browser form serialises to the
format the CLI parses — so the two cannot drift on which keys are understood. Unknown keys
become extra header fields by design.

Traps here:

- **`cover_height` and `cover_header` must agree on the row count.** They computed it separately
  at first, and the moment console/firmware rows became conditional the rule was drawn straight
  through the notes line. `header_rows()` is now the single source both use.
- **PDF objects are `Vec<u8>`, not `String`.** An image XObject's stream is raw JPEG or Flate;
  routing it through a `String` replaces every byte above 0x7F. This bit once already with the
  xref offsets — same root cause, different symptom.
- **`q`/`Q` around the image CTM.** Drawing an image means scaling the unit square, and without
  the save/restore that transform multiplies every later coordinate on the page.
- A logo that cannot be embedded is a **warning, not a failure**. The patch list is the point.

## Conventions

Follows the fleet: `AGENTS.md` is the onboarding doc, the AI-assistance disclaimer sits at the
top of `README.md`, and the "verified vs assumed" split above is the part to keep honest as the
project grows. Sample show files are client data — they belong in `patchferret-research`, never
here. The one fixture in `tests/fixtures/` is from a public repository and is safe to ship.
