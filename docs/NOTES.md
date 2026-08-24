# Notes

Working notes for this repo: status, decisions, and the traps that have actually bitten.
Migrated out of Claude Code's memory on 2026-08-24, so they are written in the first
person and dated by when each thing was learned — that date is usually the useful part.

Cross-cutting notes that are not specific to this repo live in
[fleet-notes](https://github.com/stoatworks-labs/fleet-notes).

*PatchFerret — console show file → patch list/spec/topology PDFs; LIVE preview at patchferret.stoatworks-labs.com; 5 console families ship; only DiGiCo left; conversion NOT built*

**`~/Projects/patchferret`** — github.com/stoatworks-labs/patchferret, **PUBLIC** MIT, and
**`~/Projects/patchferret-research`** — **PRIVATE**. Both created 2026-08-10, branch `main`,
CI green (test + wasm jobs).

Reads a mixing console show file → console-neutral **PFX** model → patch list, spec sheet and
wiring topology as PDFs, plus PFX interchange XML. Browser (WASM, nothing uploaded) and CLI.
146 tests. The split follows the [openrcs](https://github.com/stoatworks-labs/openrcs/blob/main/docs/NOTES.md) (`openrcs`) precedent: public tool, private RE notes.

**Scope was confirmed up front, not assumed** (**confirm scope before researching** (working-practice note, kept in Claude memory)):
modular adapters for arbitrary consoles, X32 first, then Yamaha / A&H / DiGiCo+VENUE;
client-side WASM; split public/private repos.

## The thing that makes this project non-trivial

**The patch chain has an indirection in the middle, and every console has one.** On X32:
`connector → /config/routing/IN block → input slot → /ch/NN/config source → channel`. Assuming
channel N is fed by connector N gives a patch list that is confident and **wrong** — in the test
fixture slots 25–32 are AES50-**B** 1–8, and six channels reach no connector at all. The PFX
model keeps `slot`, `socket` AND `strip` on every `InputPatch` so this survives; do not
"simplify" it away, and it is also what makes re-patching expressible later.

Two other load-bearing model decisions: **head amps key off `SocketRef`, not `StripId`** (gain
belongs to the connector and is shared by every console on a stage box), and **anything
recognised but unmodelled becomes a `Diagnostic`** — the spec sheet's fidelity section and CLI
`info` are built from those, and they're the basis of "this is what a conversion would lose".

## Yamaha DM3/DM7/TF — SHIPPED 2026-08-10

**The format is self-describing**, which is the whole reason it works: MMSXLIT payloads carry
their own schema inline (COL records = offset/datasize/arraysize, PR = type/width/arraysize).
So NO per-model tables and NO vendor descriptor files — decisive, because the browser build has
no filesystem and an adapter needing `mms_Mixing.xml` off disk could never work in WASM.
Verified against **41 factory scenes shipped inside DM3 Editor and TF Editor** (vendor content,
CANNOT be committed — unit tests use synthetic containers, integration test skips if absent).

**The trap that cost real time: Yamaha field names differ per model and getting it wrong FAILS
SILENTLY.** DM3 `Patch/Source` 4 bytes; TF `Patch/Select` 1 byte; DM7 `InPatch` with `Signal`
first. Hardcoding DM3's names made TF resolve zero connectors *and emit no diagnostic*. Find the
collection by any known name, take whatever parameter it actually holds, and treat "found the
collection, got no value" as a diagnostic. Also: **container headers are big-endian, schema
metadata and values are little-endian** — and reading DM3's patch word BE also yields tidy
ascending indices, so "looks right" proves nothing.

**TF resolves no connectors and says so** (1-byte selector, port lives in an undecoded
BuiltIn/Slot collection). Patch encoding never checked against a console's patch screen.

**CL/QL are NOT this format** — their editors ship zero descriptors, older *Lime* architecture.
Separate unstarted problem. Editors installed locally: DM3/DM7/TF/CL/QL, A&H SQ/Avantis/dLive,
and 14 DiGiCo offline apps in the Parallels Win11 VM (`C:\SD*`, `C:\Quantum*`).

## Allen & Heath Avantis/dLive — SHIPPED 2026-08-10

Show = **gzipped tar**; scenes are nested gzipped tars. Scene blobs are binary but every block
carries a readable label naming type AND object ("Parametric EQ, Input Channel 07"). Input patch
is a **`Channel Mapper`** block: 3 bytes/channel = `[type][BE u16 index, 0-based]`, type
**0x00 local, 0x03 SLink** (confirmed). 0x11 and 0x25 seen but UNIDENTIFIED — reported, not
guessed.

**Located by controlled diff driven through the GUI myself**, which is now the proven technique:
store show → change exactly ONE patch point → store again → compare. Two traps:
- **Compare DECOMPRESSED contents, not files** — gzip embeds a timestamp so every scene tarball
  differs between saves; `diff -rq` reports 60 changed files and tells you nothing.
- **The patch is NOT scene-recallable** — only in `StageBoxScene65535` (live state).

**Nearly shipped a detector that worked by coincidence:** numbered scenes lead with their own
NAME, and factory FOH happens to call scene 002 "Avantis". User-saved shows call it "Reset
Settings FOH". Nothing in the archive states the model — adapter says so.

**GUI automation notes:** Avantis Director → "Run Offline". Shows land in
`~/Library/Application Support/AllenAndHeath/AllenHeath/Avantis/Data/Director/Shows/User/`.
The patch matrix needs **Shift HELD** while clicking; on-screen Patch buttons don't arm it and
double/right-click do nothing. computer-use can't do modifier+click, but latching works:
`osascript -e 'tell application "System Events" to key down shift'` … click … `key up shift`.

**First non-workspace dep added: `miniz_oxide`** for DEFLATE (pure Rust, wasm-safe). gzip header
and tar still hand-parsed.

## CL/QL + SQ adapters — SHIPPED 2026-08-11. **Only DiGiCo remains.**

**LIVE at https://patchferret.stoatworks-labs.com**, listed on the website as "PatchFerret
(preview)" (webtools.json `group: "audio"`, no projects.json entry needed).

**CL/QL `.CLF`:** 1 byte/channel, 64 on a QL5, at **ABSOLUTE offset 0x00d74b** — nothing in the
file points at it, established on ONE QL5 + editor build. Adapter VALIDATES before trusting and
reports "probably not the table" rather than printing noise. Encoding: DANTE 1-64 = 0x01-0x40,
INPUT 1-8 = 0x41-0x48, **INPUT 9-32 = 0xC1-0xD8** — the split is real, `0x40+n` is right for 8
channels and wrong for 24.

**SQ `NVDATA.DAT`:** patch is in NVDATA **not the scene**; byte sits in a per-channel record at
**336-byte stride**; found by signature `ff ff ff [patch] 00 01 fe`, **longest run wins** (a real
image has ~5 runs; the others are different tables). Byte is a socket NUMBER with **no class** —
only Local ever observed, so the device is labelled "Input socket (class not decoded)".

**Methodological note worth keeping:** scanning for ascending byte runs found the Avantis and
CL/QL tables instantly and finds NOTHING on SQ, whose values sit 336 bytes apart. The heuristic
failing is not evidence a format lacks a patch table.

Real-file tests are **env-gated** (`PF_CLF_BASE`, `PF_SQ_NVDATA`, …) and skip cleanly — the files
carry vendor default data and are NOT committed.

## Verified vs NOT

**Verified:** X32 `.scn` adapter against a real 2,104-line scene file; all 42 output rows decode
to a known source with no fallbacks; PFX XML round-trips byte-stably; all three PDFs rendered
and visually inspected; browser build driven end to end via a real drop event.

**NOT built — don't imply otherwise:** writing show files, console-to-console conversion,
conform to a target `ConsoleProfile`. `ConsoleProfile` exists but nothing consumes it. **No
output has ever been loaded into a console.** Shipped adapters: X32, Yamaha DM3/DM7/TF, Yamaha
CL/QL, A&H Avantis/dLive, A&H SQ. **Only DiGiCo is unimplemented.** Channel NAMES are still not
decoded for CL/QL or SQ (needs its own rename-and-diff). SQ MixPad DOES keep a
CurrentShow (SCENE001.DAT + NVDATA.DAT) — an earlier note calling it control-only was wrong.
DiGiCo: 14 offline apps in the Win11 VM but NO sessions exist; one must be saved from the app.
**Website publishing:** build+deploy from a CLEAN `git worktree` at HEAD + only your files — the
shared checkout often carries a co-session's uncommitted work that a normal build would publish. X-Air is *claimed* by the X32 adapter's
extension list but **never tested** — device counts differ, so it will sniff successfully and
may be quietly wrong. Yamaha head-amp gain, bus sends and output strips are read but not yet
carried into PFX.

**Next up (user-requested):** rich PDF report header — logo, event name, date, artist/act,
venue, console, firmware, production company, engineer name + contact. Most of those are NOT in
any show file, so this needs a **job-metadata input** separate from the parsed show (browser
form / CLI sidecar), plus **image embedding** in the hand-rolled PDF writer for the logo (JPEG
passthrough as DCTDecode is the easy path; PNG needs FlateDecode + predictor). Yamaha scenes DO
supply console, version, scene Title, Comment, OwnerName and a TimeStamp for free.

## Traps already paid for (all in AGENTS.md)

- **Rust match-arm shadowing:** generic `[section, idx, "config"]` swallowed
  `["main","st","config"]`; Main/Mono strips vanished silently. `unreachable_patterns` caught it
  — never silence that lint here.
- **PDF xref offsets are byte offsets into a Latin-1 file.** Building as a `String` and
  converting at the end shifts every entry. `Document::finish` assembles `Vec<u8>`.
- **WinAnsiEncoding is NOT Latin-1** — em dash/ellipsis/curly quotes live at 0x80–0x9F, which
  Latin-1 leaves undefined. Without the mapping every em dash rendered `?`, in every empty cell.
- Biggest known gap: `/config/routing/AES50A|B|CARD|OUT` output blocks name *groups* of output
  definitions, so expanding them to per-connector rows needs a second pass. Diagnosed, not done.
  That's what would show what's actually on each stage-box return.

## Deliberate dependency choices

**Hand-rolled PDF writer** (zero deps) and **raw C ABI at the WASM boundary** (no wasm-bindgen /
wasm-pack). Both so the browser build is just `cargo build --target wasm32-unknown-unknown` —
general PDF crates drag in image decoders and fs access that don't compile there. Whole runtime
dep list is `quick-xml` + `thiserror`.

Console teardown repos ([x32 re](https://github.com/stoatworks-labs/x32-re/blob/main/docs/NOTES.md) (`x32-re`), [dm7 re](https://github.com/stoatworks-labs/dm7-re/blob/main/docs/NOTES.md) (`dm7-re`), [sq5 re](https://github.com/stoatworks-labs/sq5-re/blob/main/docs/NOTES.md) (`sq5-re`),
[yamaha ql re](https://github.com/stoatworks-labs/yamaha-ql-re/blob/main/docs/NOTES.md) (`yamaha-ql-re`)) are **firmware** work and contain no show-file material — checked.
The one crossover: **[dm7 re](https://github.com/stoatworks-labs/dm7-re/blob/main/docs/NOTES.md) (`dm7-re`) ships the DM7's whole parameter model as cleartext XML**
(2,693 params with types/ranges/packed sizes) — start Yamaha there, not in a hex editor.
