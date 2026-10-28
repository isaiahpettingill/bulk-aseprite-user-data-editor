# Aseprite library assessment

Evaluated 2026-10-27 against published `aseprite-io` 0.2.0 (crate VCS commit
`6150416149883784a7a3b302a10dcd03d188e91c`). This is a narrow compatibility review,
not a security audit or a judgment about the maintainers.

## Candidates

- [asefile](https://github.com/alpine-alpaca/asefile), 0.3.8, MIT: useful independent
  image decoder and metadata reader. No writer; user data exposes text/color, not
  the full typed property maps. Used in regression tests, not as our save path.
- [aseprite-io](https://github.com/spebern/aseprite-io), 0.2.0, MIT OR Apache-2.0:
  real reader/writer, including typed properties, tags, slices and tilesets.
  Rust 1.87 minimum; default dependency is `flate2`. Its changelog dates 0.1.0 to
  2026-03-26 and 0.2.0 to 2026-06-30. Default-feature code successfully compiled
  for `wasm32-unknown-unknown` in this assessment.

## Verified strengths of aseprite-io 0.2.0

The reader stores unknown chunks and their ordering, and retains original zlib
streams for compressed cels/tilesets. A local generated-fixture test confirmed
that an existing sprite user-data text edit survives write/read, an unknown
chunk's payload survives, and compressed cel bytes are unchanged.

## Reasons it is not currently a drop-in metadata save engine

1. **Adding missing metadata to an imported file is lost.** The public
   `set_sprite_user_data` setter only sets the data value. For an imported file,
   the writer follows the original chunk-order list without creating a missing
   user-data entry. A local fixture with a palette/layer but no user-data chunk
   was read, given sprite text, written, and read again: sprite user data was
   absent. This is a core bulk-append use case, not an optional edge case.
2. **Existing tag names cannot be changed through the public API.** `tags()` is
   read-only and `tags_mut()` is crate-private. Adding tags is supported, as is
   setting existing tag user data; replacing existing names/ranges is not exposed.
3. **Byte-perfect preservation is not universal.** Known chunks are reconstructed,
   reserved bytes are zeroed and header pixel-ratio zeros are normalized to ones.
   Our fixture retained a cel's compressed stream but not its reserved bytes.
   Unknown future data should not be silently discarded by a metadata-only tool.
4. **Untrusted-input limits need a wrapper or upstream work.** The reader uses
   unbounded zlib `read_to_end`, allocates property/vector capacities from file
   counts, and recurses into nested properties without an explicit depth bound.
   A local malformed five-byte chunk triggered a subtraction-overflow panic in
   debug mode (`chunk_size - 6`). This review did not attempt resource-exhaustion
   tests or assert a published vulnerability.

Relevant source:
[reader](https://github.com/spebern/aseprite-io/blob/6150416149883784a7a3b302a10dcd03d188e91c/src/reader.rs),
[writer](https://github.com/spebern/aseprite-io/blob/6150416149883784a7a3b302a10dcd03d188e91c/src/writer.rs),
[types and setters](https://github.com/spebern/aseprite-io/blob/6150416149883784a7a3b302a10dcd03d188e91c/src/types.rs),
[API documentation](https://docs.rs/aseprite-io/0.2.0/aseprite/).

## Recommendation

Do not replace the narrowly scoped, bounded chunk editor with the 0.2.0 writer
without first resolving these tested gaps. Prefer existing libraries where they
provide the required guarantees: `asefile` already independently checks decoded
artwork in tests. Reassess `aseprite-io` after missing-metadata insertion,
public tag editing, safe-input limits and preservation regression tests land.
No upstream issues, patches or public messages were submitted during this review.

## Reproduce

`cargo run --manifest-path tools/library-probe/Cargo.toml` exercises only
small generated fixtures. It prints each observed result; the caught malformed
chunk panic also invokes Rust's normal panic hook. This optional probe is not
a production dependency or part of normal CI.

## Parser-only candidate: aseprite-loader 0.4.2

[aseprite-loader](https://github.com/bikeshedder/aseprite-loader) is MIT OR
Apache-2.0, Rust 1.70+, and **does expose typed user-data properties** in its public
`binary` module. It must not be dismissed simply because it has no writer.
Inspected published source commit `8c54750aa8137527c0f92df4bd3dc2cfe5da9fe8`.

Useful APIs include `binary::chunks::tags::parse_tags_chunk`,
`binary::chunks::layer::parse_layer_chunk`, and
`binary::chunks::user_data::parse_user_data_chunk`. Raw cel parsing borrows
compressed bytes rather than decompressing them. These can fit a metadata-only
editor paired with a surgical writer and retained raw spans.

Verified limitations:

- Raw frame/chunk parsers subtract four from untrusted lengths without checked
  subtraction. `parse_raw_file` also does not reconcile the header frame count.
- `Chunk::Unsupported` keeps the unknown type, not its original payload; callers
  still need original bytes/spans for lossless saves.
- A generated flat string-property fixture parsed successfully. A spec-correct
  nested-map property failed: property type 0x12 delegates to the top-level map
  parser, which expects an extension-entry ID that nested maps do not contain.
  This appears as `properties_maps: Some(Err(...))` inside an outer successful
  user-data parse, so callers must inspect the nested result.
- A property-block size of seven produced a debug underflow panic (`size_maps - 8`).
  The nested parser has no explicit recursion-depth limit.
- Layer flags for prefer-linked/collapsed/reference are declared as `0x16`,
  `0x32`, `0x64`, rather than the spec's decimal `16`, `32`, `64`. Using only its
  name parser does not depend on those flags; rebuilding layers from those flags
  would be inappropriate.

Exact small malformed input used for the property-size test:
`[4,0,0,0, 7,0,0,0, 0,0,0,0]` passed to `parse_user_data_chunk`.
The valid nested test encoded one map with extension ID zero, one property `x`
of type 0x12, then one nested property `a` of type string containing `yes`.
The corresponding flat test replaced the nested value with the string `yes`.

Source:
[user-data parser](https://github.com/bikeshedder/aseprite-loader/blob/8c54750aa8137527c0f92df4bd3dc2cfe5da9fe8/src/binary/chunks/user_data.rs),
[raw frame parser](https://github.com/bikeshedder/aseprite-loader/blob/8c54750aa8137527c0f92df4bd3dc2cfe5da9fe8/src/binary/raw_frame.rs),
[chunk parser](https://github.com/bikeshedder/aseprite-loader/blob/8c54750aa8137527c0f92df4bd3dc2cfe5da9fe8/src/binary/chunk.rs),
[layer parser](https://github.com/bikeshedder/aseprite-loader/blob/8c54750aa8137527c0f92df4bd3dc2cfe5da9fe8/src/binary/chunks/layer.rs).

Recommended narrow integration: keep bounded container validation and original
chunk bytes; use suitable library leaf parsers for tags and supported text/color
metadata after preflight, preserving unknown flags and opaque properties. Do not
use its nested-property interpretation or layer flags to rewrite data until
corrected. This is a production-parser reuse option, not only a test oracle.

## Implemented hybrid

The production dependency is now pinned to `aseprite-loader = "=0.4.2"`.
`src/aseprite.rs` uses its tag parser, layer-name parser and user-data text/color
parser. Original frame/chunk boundaries and payloads remain under our bounded
preservation layer. Tag counts are checked against both resource limits and
minimum available bytes before library allocation. Text/color fields are length
checked and passed as an isolated prefix with the properties bit masked out;
this never enters the library's defective nested-property parser. Future flags
remain visible and read-only. Typed property interpretation and the surgical
writer remain local, with depth/count limits and opaque unsupported-data
preservation. The library's layer flags are never used to rebuild layers.

Regression tests exercise truncated layer/tag/user-data fields, excessive tag
counts, malformed property lengths, future flags, nested properties, and
independent `asefile` decoding with unchanged RGBA/indexed pixels.

## Read-only sprite preview

`asefile = "=0.3.8"` now also renders the focused sprite's first frame.
The preview adapter validates dimensions, chunk/layer counts, palette ranges and
a bounded decoded-pixel budget, then supplies only supported first-frame drawing
chunks to that library. Compressed cel output is bounded before library decoding.
Typed/unknown metadata never enters the preview renderer; original document
bytes and surgical writes remain independent. Tilemaps and linked first-frame
cels are currently unsupported previews, not blocked metadata edits.

Preview limits: 16 MiB input, 1024 pixels per dimension, 128 layers, 4096 chunks,
and 2 Mi decoded cel pixels. Only the focused file is decoded, with a single
cached nearest-neighbor texture; typing and saving metadata do not re-render it.
