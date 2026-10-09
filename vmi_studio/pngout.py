"""Scale a picture before it is written. 100% is the original image."""

from PIL import Image


def scaled_size(width, height, percent):
    """Pixel size at this percentage of the source. At least one pixel."""
    try:
        pct = int(percent)
    except (TypeError, ValueError):
        pct = 100
    pct = max(1, pct)
    if pct == 100:
        return int(width), int(height)
    return (
        max(1, int(round(float(width) * pct / 100.0))),
        max(1, int(round(float(height) * pct / 100.0))),
    )


def scale_image(image, percent, resample="nearest"):
    """The same image at 100%. Otherwise nearest or bicubic."""
    if image is None:
        return None
    try:
        pct = int(percent)
    except (TypeError, ValueError):
        pct = 100
    if pct == 100:
        return image
    width, height = scaled_size(image.size[0], image.size[1], pct)
    method = Image.Resampling.BICUBIC if str(resample).lower() == "bicubic" else Image.Resampling.NEAREST
    return image.resize((width, height), method)
