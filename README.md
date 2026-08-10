# PatchFerret

> **AI-assisted project.** This codebase was created with [Claude Code](https://claude.com/claude-code)
> (Anthropic), directed and reviewed by a human author. The Behringer X32 adapter is written
> against a real 2,104-line scene file saved by an actual console and is asserted against it in
> CI — 90 tests across the workspace, of which 17 cover the X32 adapter: the
> connector→slot→channel composition, the head-amp index split across the local and both AES50
> links, and the output-source enumeration. The generated PDFs are
> produced and visually inspected. **No output has ever been loaded back into a console**, no
> conversion between consoles exists yet, and every format other than the X32 `.scn` is
> unimplemented. The Yamaha adapter is checked against the 41 factory scenes shipped inside DM3
> and TF Editor, but its **patch-source encoding has never been confirmed against a console's
> patch screen**, and on TF no connector resolves at all — the tool says so rather than
> guessing. The X32 signal-source enumerations are likewise derived from community documentation
> and corroborated against real files, **not** from a running desk — see
> [Provenance](#provenance-of-the-x32-enumerations).

Turn a mixing console show file into documentation an engineer can actually use: a patch list,
a specification sheet and a wiring topology diagram, as PDFs.

Runs in the browser or on the command line. The browser build parses and renders entirely in
WebAssembly, so **a show file never leaves the machine** — there is no upload and no server.

## What it does today

```
show file  →  format adapter  →  PFX model  →  ├─ patch list PDF
                                               ├─ specification PDF
                                               ├─ topology PDF
                                               └─ PFX interchange XML
```

| Console | File | Status |
|---|---|---|
| Behringer X32 / Midas M32 / X-Air | `.scn` | **Supported** |
| Yamaha DM3 / DM7 / TF | `.dm3s` `.tfs` `.dm7s` (+ presets) | **Supported** — names and input patch; head amps and sends not yet |
| Yamaha CL / QL | `.CLF` | Not started — a different, older format |
| Allen & Heath Avantis / dLive | `.tar.gz` | **Supported** — input patch and strip inventory; names and preamps not yet |
| Allen & Heath SQ | — | Planned |
| DiGiCo, Avid VENUE | — | Planned |

Format research for the unimplemented consoles lives in the private `patchferret-research`
repository, along with sample show files (which are client data and do not belong in a public
repo).

## Use it

### Command line

```bash
cargo build --release
./target/release/patchferret info   myshow.scn
./target/release/patchferret report myshow.scn -o ./docs
./target/release/patchferret xml    myshow.scn -o myshow.pfx.xml
```

`info` prints a summary and, importantly, the **fidelity list** — everything the adapter read
but could not carry into the model, and which would therefore be lost in a conversion.

### The report header

Reports carry a header with your logo, the event, venue, engineer and so on. **None of that is
in a show file** — a console stores a mixer state, not the job it was built for — so it comes
from a job sheet you supply:

```bash
patchferret job-template -o job.txt   # a starting point
patchferret report myshow.scn -j job.txt -o ./docs
```

The sheet is `key: value` lines, and **any key it does not recognise becomes an extra header
field**, so "Truck call" or "Rider rev" work without the tool knowing about them:

```
Event: Summer Live 2026
Date: 12-14 September
Artist: The Something Band
Venue: Old Granada Studios
Engineer: A. Sargeant
Contact: 07700 900000
Truck call: 06:00
logo: ./logo.jpg
```

The browser version has the same fields as a form, and accepts **any image** for the logo —
it converts on a canvas before handing the bytes over. The CLI takes JPEG, or PNG without
transparency; embedding a transparent PNG would mean decoding pixels, which the dependency-free
PDF writer deliberately does not do. It says so rather than dropping the logo silently.

### Browser

```bash
rustup target add wasm32-unknown-unknown
./scripts/build-web.sh
python3 -m http.server 8731 --directory web
```

Then open <http://localhost:8731>. Module scripts and WASM will not load from `file://`, so it
has to be served over HTTP.

## Why the patch list is not obvious

On an X32, getting from an XLR to a fader takes three hops, and all three have to be composed:

1. `/config/routing/IN` maps blocks of **eight physical connectors** onto the 32 **input slots**.
2. `/ch/NN/config` selects which **input slot** feeds channel `NN` — a free mapping.
3. `/headamp/NNN` holds the gain for a **connector**, in a flat index across local and both
   AES50 links.

Assuming channel *N* is fed by XLR *N* produces a confident and wrong patch list. In the test
fixture, slots 25–32 are AES50-**B** connectors 1–8 while channels 25–32 carry them, and six
channels reach no connector at all. PatchFerret composes the chain and marks the dead rows.

## The PFX model

`patchferret-model` defines a console-neutral show model and its XML serialisation. The design
rule is that the model is **lossy in a declared way**: anything an adapter understood but the
model cannot express becomes a `Diagnostic`, which the spec sheet prints. A show that converts
with no diagnostics is one we can claim to have fully understood; anything else says so.

Two choices worth knowing:

- **Head amps hang off the socket, not the channel.** On a shared stage box the gain belongs to
  the connector and every console listening to it shares that gain. Modelling it per-channel is
  the standard way to produce a patch list that lies about gain sharing.
- **The input-slot indirection is preserved.** Resolving it away at parse time would make the
  connector column a guess and would make re-patching impossible to express later.

`ConsoleProfile` describes what a desk *has* — channel counts, bus counts, EQ bands, physical
I/O. It is the prerequisite for the conform work below, and is populated now so the shape is
fixed before adapters depend on it.

## Provenance of the X32 enumerations

The signal-source numbering is not published by the vendor. It is derived from community
documentation and corroborated against real scene files. Two internal checks support it, and
both are asserted in CI:

- No value in the decoded range resolves to a matrix above 6. The X32 has exactly six, so the
  competing "1–16 are the mix buses" reading is ruled out — it would require matrix 7 and 8.
- `/outputs/p16/01 26 <-EQ` decodes to "direct out of channel 1, pre-EQ", which is what an
  Ultranet port 1 conventionally carries.

Every output row in the test fixture decodes cleanly, which is asserted as a test. Values
outside the mapped ranges are reported as diagnostics rather than guessed.

## Not built yet

The longer-term goals, in dependency order:

1. **Editing the patch** — re-assign connector→channel and write the show file back out. The
   model already keeps the slot indirection this needs; no adapter can write yet.
2. **Console-to-console conversion** — read one format, write another via PFX.
3. **Conform** — reconcile a show against a target `ConsoleProfile` and report what does not
   fit, with intelligent mapping where a straight copy is impossible.

Each of these needs a *writing* adapter, which is strictly harder than reading, and needs
hardware to verify against. None of it should be trusted until a converted show has been loaded
into a real console.

## Layout

```
crates/patchferret-model     PFX model, XML, console profiles
crates/patchferret-formats   adapter trait, registry, X32 / Yamaha / A&H adapters
crates/patchferret-report    dependency-free PDF writer and the three reports
crates/patchferret-cli       the local tool
crates/patchferret-wasm      C-ABI entry point for the browser
web/                         browser front end
```

See [AGENTS.md](AGENTS.md) for the onboarding detail.

## Licence

MIT.

X32, M32, and the console names in this repository are trademarks of their respective owners.
PatchFerret contains no vendor code and is not affiliated with or endorsed by any console
manufacturer.
