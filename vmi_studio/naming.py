"""Layer names from the VMI grammar. SSSXXX, hex state and frame."""

import re


STATE_LABELS = {0: "rest", 1: "hover", 2: "pressed", 3: "after"}

LABEL_TO_STATE = {
    "default": 0,
    "idle": 0,
    "normal": 0,
    "rest": 0,
    "hover": 1,
    "active": 1,
    "pressed": 2,
    "press": 2,
    "down": 2,
    "after": 3,
}

OMIT_RE = re.compile(
    r"^(?:ref(?:[_\-\s]|$)|sketch(?:[_\-\s]|$)|guide(?:[_\-\s]|$)|uigrid|paper\b|_|\s*\[ref\])"
    r"|(?:\b(?:reference|uigrid|guide|sketch|template)\b)",
    re.IGNORECASE,
)
STATE_RE = re.compile(
    r"(?:#|\.|__|_)(hover|pressed|press|after|active|down|idle|normal|default|rest|s[0-9a-fA-F]{1,3})\b"
)
FRAME_RE = re.compile(r"(?:@f|[#._]f|frame[_-]?)([0-9]+)\b", re.IGNORECASE)
VAGUE_RE = re.compile(
    r"^(?:\d+|(?:layer|folder|group|copy)(?:\s*\d+)?|(?:レイヤー|コピー)(?:\s*\d+)?)$",
    re.IGNORECASE,
)


def is_omitted_name(name):
    return bool(OMIT_RE.search((name or "").strip()))


def is_vague_name(name):
    return bool(VAGUE_RE.match((name or "").strip()))


def sanitize(name):
    text = STATE_RE.sub("", name or "")
    text = FRAME_RE.sub("", text)
    text = re.sub(r"[#@]+$", "", text).strip(" ._-/")
    text = re.sub(r'[<>:"/\\|?*]+', "_", text)
    return text or "layer"


def parse_sf(raw):
    """Return (state, frame), each masked to 12 bits."""
    state = 0
    frame = 0
    state_match = STATE_RE.search(raw or "")
    if state_match:
        token = state_match.group(1).lower()
        if token.startswith("s") and all(char in "0123456789abcdef" for char in token[1:]):
            state = int(token[1:], 16)
        else:
            state = LABEL_TO_STATE.get(token, 0)
    frame_match = FRAME_RE.search(raw or "")
    if frame_match:
        frame = int(frame_match.group(1))
    return state & 0xFFF, frame & 0xFFF


def slot_key(state, frame):
    return "%03X%03X" % (state & 0xFFF, frame & 0xFFF)


def state_label(state):
    return STATE_LABELS.get(state & 0xFFF, "custom")


def folder_name(name):
    clean = re.sub(r'[<>:"/\\|?*]+', " ", name or "")
    clean = re.sub(r"\s+", " ", clean).strip()
    return clean or "object"
