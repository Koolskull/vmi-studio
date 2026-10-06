"""Convert a .clip file to a layered PSD using the vendored clip_to_psd."""

import os
import sys


def project_root():
    return os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def third_party_dir():
    return os.path.join(project_root(), "third_party")


def convert_clip_to_psd(source, destination, blank_preview=True, log_level="WARNING"):
    """Write destination as a PSD. Overwrites an existing file."""
    source = os.path.abspath(source)
    destination = os.path.abspath(destination)
    if not os.path.isfile(source):
        raise FileNotFoundError(source)
    os.makedirs(os.path.dirname(destination) or ".", exist_ok=True)
    vendor = third_party_dir()
    if vendor not in sys.path:
        sys.path.insert(0, vendor)
    import clip_to_psd

    argv = ["clip_to_psd", source, "-o", destination, "--log-level", log_level]
    if blank_preview:
        argv.append("--blank-psd-preview")
    previous = sys.argv
    try:
        sys.argv = argv
        clip_to_psd.main()
    except SystemExit as exc:
        if exc.code not in (0, None):
            raise RuntimeError("clip to psd failed (%s)" % exc.code) from exc
    finally:
        sys.argv = previous
    if not os.path.isfile(destination) or os.path.getsize(destination) < 26:
        raise RuntimeError("clip to psd wrote no PSD at %s" % destination)
    return destination
