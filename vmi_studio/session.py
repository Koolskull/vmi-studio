"""Remember the open drawing so a restarted window comes back to it."""

import json
import os


PATH = os.path.join(os.path.expanduser("~"), ".cache", "vmi-studio", "session.json")


def load():
    try:
        with open(PATH, encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def save(data):
    os.makedirs(os.path.dirname(PATH), exist_ok=True)
    temp = PATH + ".tmp"
    with open(temp, "w", encoding="utf-8") as handle:
        json.dump(data, handle)
    os.replace(temp, PATH)
