# Replace `Parsers` enum with `Box<dyn Parser>`

## Problem
`src/parser.rs` defines a `Parsers` enum wrapping each concrete parser
(`grep`, `gpx`, `tt_json`, `kml`, `json`, `geojson`). Every `Parser`
trait method is then implemented by match-dispatching across all
variants — `parse`, `finalize`, `set_layer_name`, etc. Adding a parser
requires editing every match in the file.

## Direction
- Keep the `Parser` trait as is.
- Drop the `Parsers` enum.
- Store parsers as `Box<dyn Parser>` (or `Arc<dyn Parser>` if shared).
- Construct the active parser chain from CLI args by pushing boxed
  instances into a `Vec<Box<dyn Parser>>`.

## Expected payoff
- ~50 lines of boilerplate gone.
- Adding a parser is a single new file + one registration line.
- Trait-object dispatch cost is irrelevant here (parser runs per input
  line, not per frame).

## Risk
Low. Mechanical refactor with existing parser tests as a safety net.
