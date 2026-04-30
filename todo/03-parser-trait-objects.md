# Replace `Parsers` enum with `Box<dyn Parser>`

## Problem
`src/parser.rs` defines a `Parsers` enum wrapping each concrete parser
(`grep`, `gpx`, `tt_json`, `kml`, `json`, `geojson`). Every `Parser`
trait method is then implemented by match-dispatching across all
variants — `parse`, `finalize`, `set_layer_name`, etc. Adding a parser
requires editing every match in the file.

## Constraint
`Parsers` is stored as a field in `ExeCfg` and `CurlCfg` in
`external_cmd.rs`, both of which derive `Serialize + Deserialize`. These
configs are persisted to disk. `Box<dyn Parser>` can't derive those
traits, so a naive drop-in is blocked.

## Revised direction
Two viable paths:

**A. Keep enum, eliminate impl boilerplate.**
Use a macro or a blanket delegation proc-macro to reduce the three
match blocks to one. The enum stays serializable; boilerplate is hidden.
Modest payoff.

**B. Split into serializable config + runtime parser.**
Introduce a `ParserConfig` enum (just the discriminant + config data,
fully serializable) and a separate `Box<dyn Parser>` at runtime. Convert
`ParserConfig → Box<dyn Parser>` when executing a command.
Full payoff: no match boilerplate, easy to extend.

Path B is the right long-term design. It also decouples config schema
from parser implementation — useful when parsers carry mutable state
that shouldn't be serialized (e.g., partial JSON accumulation in
`JsonParser`).

## Expected payoff (path B)
- ~50 lines of match boilerplate gone from `parser.rs`.
- `ExeCfg`/`CurlCfg` serialize only config, not parser state.
- Adding a parser: one new variant in `ParserConfig`, one `impl Parser`.

## Risk
Medium. Touches serialized config format — existing saved command
configs must still deserialize correctly (use `#[serde(rename)]` if
variant names change).
