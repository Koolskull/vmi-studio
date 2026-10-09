# vmi-clip-vector

Small reader for Clip Studio vector layers. VMI STUDIO bakes the JSON into a preview bitmap. The reader is [clipfile](https://github.com/Aodaruma/clipfile-rs) with the `sqlite` feature (`VectorObjectList`, then `read_vector`). Brush anti-alias and hardness come from `BrushStyle`. Fill strokes use `FillStyle.anti_alias`.

```
cargo build --manifest-path tools/vmi-clip-vector/Cargo.toml --release
tools/vmi-clip-vector/target/release/vmi-clip-vector drawing.clip
```

`VMI_CLIP_VECTOR` overrides the binary path. A rejected vector body is reported on stderr and that layer stays blank. SVG is not a fallback.

The stroke lookup id is `brush_style_id` (the `BrushStyle` row). `brush_id()` is a different field and is not that row. Older files omit `BrushStyle.MipmapIndexToPlot`. The reader then selects `AntiAlias` and `Hardness` by `MainId`.

Raw `anti_alias` is logged as `vector alias raw=<n>`. `0` is None (hard edge). Other values are anti-aliased. `2` and `3` have both been read from real files and still share the unmapped fringe. Weak, Middle, and Strong stay unmapped until a known None file and a known Strong file have both been logged.
