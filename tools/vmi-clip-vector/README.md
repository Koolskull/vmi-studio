# vmi-clip-vector

Small reader for Clip Studio vector layers. VMI STUDIO bakes the JSON into a preview bitmap. The reader is [clipfile](https://github.com/Aodaruma/clipfile-rs) with the `sqlite` feature (`VectorObjectList`, then `read_vector`). Brush anti-alias and hardness come from `BrushStyle`. Fill strokes use `FillStyle.anti_alias`.

```
cargo build --manifest-path tools/vmi-clip-vector/Cargo.toml --release
tools/vmi-clip-vector/target/release/vmi-clip-vector drawing.clip
```

`VMI_CLIP_VECTOR` overrides the binary path. A rejected vector body is reported on stderr and that layer stays blank. SVG is not a fallback.

The stroke lookup id is `brush_style_id` (the `BrushStyle` row). `brush_id()` is a different field and is not that row. Older files omit `BrushStyle.MipmapIndexToPlot`. The reader then selects `AntiAlias`, `Hardness`, and `CompositeMode` by `MainId`.

`composite` 27 is Clip Studio ink Erase: a transparent vector stroke. The preview cuts ink already drawn on that layer. It does not paint the stored color. Other modes paint. A missing mode paints.

Each point includes `width_scale`, the f32 at point byte 36. Untapered strokes store 1. Pen strokes move between 0 and 1 and are thin at the ends. The preview multiplies it by `width_factor`. It is not averaged across the stroke.

Raw `anti_alias` is logged as `vector alias raw=<n>`. `0` is None (hard edge). Other values are anti-aliased. `2` and `3` have both been read from real files and still share the unmapped fringe. Weak, Middle, and Strong stay unmapped until a known None file and a known Strong file have both been logged.
