# Vendored readers

VMI STUDIO uses these files to open Clip Studio `.clip` files and Photoshop `.psd` / `.psb` files.

- `src/clip2krita/` is from clip2krita. MIT License, Copyright (c) 2026 koolskull. See `LICENSE`.
- `third_party/clip_to_psd.py` converts a `.clip` into a PSD. MIT License, Copyright (c) 2024. See `third_party/LICENSE`.

The studio calls `convert_clip_to_psd` and `psdstack.load_stack`. The rest of clip2krita is not included.
