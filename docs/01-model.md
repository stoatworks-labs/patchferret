# The PFX model

PFX — PatchFerret eXchange — is the console-neutral description of a show that sits between the
format adapters and everything else. This document is the reference for the XML form; the Rust
types in `patchferret-model` are the normative definition.

Namespace: `https://patchferret.stoatworks.dev/schema/1`

## Shape

```xml
<pfx xmlns="https://patchferret.stoatworks.dev/schema/1" version="1">
  <show name="General 1.2.2" console="Behringer X32 / Midas M32"
        source-format="x32" format-version="2.7"/>
  <devices>
    <device id="aes50a" label="AES50 A" transport="aes50a" inputs="48" outputs="48"/>
  </devices>
  <headamps>
    <headamp socket="aes50a/in/1" gain-db="32.0" phantom="false"/>
  </headamps>
  <strips>
    <strip id="input/1" name="Diazno" source="slot:1" fader-db="2.1" colour="1" icon="CY"/>
  </strips>
  <patch>
    <inputs>
      <in slot="1" block="A1-8" socket="aes50a/in/1" strip="input/1"/>
    </inputs>
    <outputs>
      <out socket="local/out/1" source="strip:bus/1" tap="pre-fader" source-label="Bus 1"/>
    </outputs>
  </patch>
  <scenes/>
  <diagnostics>
    <diagnostic severity="unmodelled" locus="/config/routing/IN">…</diagnostic>
  </diagnostics>
</pfx>
```

## References

Two string forms appear as attribute values and are parsed structurally.

**`SocketRef`** — `device/direction/index`, e.g. `aes50a/in/17`. The index is 1-based and matches
what is printed on the box. Direction is `in` or `out`, from the console's point of view.

**`StripId`** — `kind/index`, e.g. `bus/4`. Kinds are `input`, `auxin`, `fxreturn`, `bus`,
`matrix`, `main`, `mono`, `dca`, `mutegroup`.

**`SignalRef`** — a tagged string, one of:

| Form | Meaning |
|---|---|
| `off` | nothing patched |
| `slot:N` | console input slot N, before channel assignment |
| `socket:device/dir/N` | a physical connector |
| `strip:kind/N` | a tap off another strip |
| `named:TEXT` | understood as a name but not resolvable |

## The slot indirection

The single most important structural decision. Consoles interpose a re-patchable stage between
the physical connector and the channel, so `<in>` carries three things at once:

- `slot` — the console's input slot number
- `socket` — the connector currently feeding that slot
- `strip` — the strip currently taking that slot

Collapsing these into a single connector→channel pair would lose the ability to express
re-patching, and would make the connector column a guess whenever a console's routing block is
set to something other than the identity mapping.

## Head amps

`<headamp>` is keyed by `socket`, never by strip. Preamp gain is a property of the connector.
Where a stage box is shared between consoles, that gain is shared with it — a model that hangs
gain off the channel cannot represent this and will produce a patch list that misleads.

Adapters are expected to emit head amps only for connectors the show actually patches. A console
stores its full complement regardless of use, and listing all 128 buries the eight that matter.

## Diagnostics are output, not debug

`<diagnostic>` records anything an adapter recognised but could not carry into the model:

| Severity | Meaning |
|---|---|
| `unmodelled` | understood, but PFX has no field for it — will not survive conversion |
| `suspect` | recognised, but the value made no sense |
| `unknown` | not recognised at all |

The spec-sheet PDF's fidelity section and the CLI's `info` output are built from these. A show
that parses with no diagnostics is one the adapter fully understood; anything else is explicit
about the gap. Silently dropping a recognised element is a bug.

## Console profiles

`ConsoleProfile` is separate from any show: it describes what a desk *has* — channel, bus,
matrix and DCA counts, EQ bands, preamp count, and physical I/O devices. It exists so that
conforming a show to a different console is a data problem rather than an N×N grid of bespoke
converters. Nothing consumes it yet.

## Versioning

The `version` attribute on `<pfx>` is checked on read; a mismatch is an error rather than a
best-effort parse. `xml.rs` is hand-written specifically so that renaming a Rust field cannot
silently change this wire format, and round-trip tests assert that serialisation is stable.
