"""An exposure sheet: ordered slots pointing at a pool of frame ids.

A hold is several slots and one pool frame. An empty cell is not a hold.
Frame ids are permanent hex, the same three digits as the SSSXXX frame half.
Boil follows that frame, not the playback clock. Onion is the previous and
next pool ids.

Cell timing follows OpenToonz TXsheet: reverse, swing, step, each, reframe,
reset step, increase step, decrease step, autoexpose, and repeat. Step
repeats every cell. Reframe collapses a run first.
"""

import random


FRAME_MAX = 0xFFF


def _frame_id(frame):
    """Three hex digits, the same shape as a frame id. Nothing is allocated."""
    if not isinstance(frame, str):
        raise ValueError("frame id is 3 hex digits")
    token = frame.strip().upper()
    if len(token) != 3 or any(char not in "0123456789ABCDEF" for char in token):
        raise ValueError("frame id is 3 hex digits")
    return token


def _id_list(raw):
    if isinstance(raw, (str, bytes)) or not isinstance(raw, (list, tuple)):
        raise ValueError("frame id is 3 hex digits")
    return [_frame_id(item) for item in raw]


def _cell_list(raw):
    """Slots may be blank. A blank is not a pool id."""
    if isinstance(raw, (str, bytes)) or not isinstance(raw, (list, tuple)):
        raise ValueError("frame id is 3 hex digits")
    cells = []
    for item in raw:
        if item == "" or item is None:
            cells.append("")
            continue
        cells.append(_frame_id(item))
    return cells


def _drawing_number(cell):
    if not cell:
        return -1
    return int(cell, 16)


def _plain_int(value, message):
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(message)
    return value


class Onion:
    """Previous and next pool ids for one slot. No pixels."""

    def __init__(self, prev, nxt):
        self.prev = prev
        self.next = nxt

    def __eq__(self, other):
        return (
            isinstance(other, Onion)
            and self.prev == other.prev
            and self.next == other.next
        )

    def __repr__(self):
        return "Onion(prev=%r, next=%r)" % (self.prev, self.next)


class ExposureSheet:
    """Slots are the timeline. The pool is the drawings those slots show."""

    def __init__(self, base_seed=0):
        self.slots = []
        self.pool = []
        self._issued = 0
        self.base_seed = int(base_seed) & 0xFFFFFFFF
        self.active = 0

    def __len__(self):
        return len(self.slots)

    def __getitem__(self, index):
        return self.slots[index]

    def __repr__(self):
        return "ExposureSheet(slots=%r, pool=%r)" % (self.slots, self.pool)

    def _slot(self, index):
        if not self.slots:
            raise IndexError("empty sheet")
        if index is None:
            index = self.active
        index = int(index)
        if index < 0 or index >= len(self.slots):
            raise IndexError("no exposure at %s" % index)
        return index

    def _alloc(self):
        """Next permanent id. Issued ids are never used for a later frame."""
        if self._issued > FRAME_MAX:
            raise ValueError("no frame ids left")
        ident = "%03X" % self._issued
        self._issued += 1
        self.pool.append(ident)
        return ident

    def _parse(self, frame):
        token = str(frame or "").strip().upper()
        if len(token) != 3 or any(char not in "0123456789ABCDEF" for char in token):
            raise ValueError("frame id is 3 hex digits")
        if token not in self.pool:
            raise ValueError("unknown pool frame")
        return token

    def append_blank(self):
        """A new blank pool frame on the end of the sheet."""
        ident = self._alloc()
        self.slots.append(ident)
        self.active = len(self.slots) - 1
        return ident

    def hold(self, index=None, count=1):
        """`count` more slots on the same pool frame. No new drawing."""
        count = int(count)
        if count < 1:
            raise ValueError("hold needs a slot")
        index = self._slot(index)
        ident = self.slots[index]
        if not ident:
            raise ValueError("empty cell")
        for step in range(1, count + 1):
            self.slots.insert(index + step, ident)
        self.active = index + count
        return ident

    def duplicate(self, index=None):
        """A new pool frame inserted after this slot. Not a second pointer."""
        index = self._slot(index)
        if not self.slots[index]:
            raise ValueError("empty cell")
        ident = self._alloc()
        self.slots.insert(index + 1, ident)
        self.active = index + 1
        return ident

    def reuse(self, frame, index=None):
        """Point a slot at a pool frame that already exists."""
        ident = self._parse(frame)
        index = self._slot(index)
        self.slots[index] = ident
        self.active = index
        return ident

    def delete(self, index=None):
        """Drop a slot. A pool frame stays while any slot still shows it.

        An id with no slot left stays in the pool so it can be reused.
        The last slot on the sheet is left in place.
        """
        if len(self.slots) <= 1:
            return None
        index = self._slot(index)
        ident = self.slots.pop(index)
        self.active = max(0, index - 1)
        return ident

    def move(self, src, dst):
        """Move one slot. The pool order does not follow."""
        src = self._slot(src)
        dst = int(dst)
        if dst < 0 or dst >= len(self.slots):
            raise IndexError("no exposure at %s" % dst)
        if src == dst:
            self.active = dst
            return self.slots[dst]
        ident = self.slots.pop(src)
        self.slots.insert(dst, ident)
        self.active = dst
        return ident

    def boil_seed(self, index):
        """Seed for this exposure. Same pool frame, same seed.

        The index picks the slot. Playback time is not an input, so a hold
        does not boil again on each tick.
        """
        ident = self.slots[self._slot(index)]
        if not ident:
            raise ValueError("empty cell")
        return (self.base_seed + int(ident, 16)) & 0xFFFFFFFF

    def _shown(self, index):
        if index < 0 or index >= len(self.slots):
            return None
        return self.slots[index] or None

    def onion(self, index=None):
        """Previous and next pool ids around this slot. A blank neighbor is none."""
        index = self._slot(index)
        return Onion(self._shown(index - 1), self._shown(index + 1))

    def slot_state(self, index):
        """'hold' continues the previous id. 'reuse' points back at an earlier one.

        A blank is neither. The same id with a blank between them is a reuse.
        """
        try:
            index = int(index)
        except (TypeError, ValueError):
            return ""
        if index < 0 or index >= len(self.slots):
            return ""
        ident = self.slots[index]
        if not ident:
            return ""
        if index > 0 and self.slots[index - 1] == ident:
            return "hold"
        for earlier in self.slots[:index]:
            if earlier == ident:
                return "reuse"
        return ""

    def _span(self, start, count):
        start = int(start)
        count = int(count)
        if count < 1 or start < 0 or start + count > len(self.slots):
            raise IndexError("no exposure at %s" % start)
        return start, count

    def insert_empty(self, index, count=1):
        """OpenToonz insert: empty cells at this row, and the rest shift down."""
        count = int(count)
        if count < 1:
            raise ValueError("insert needs a cell")
        index = int(index)
        if index < 0 or index > len(self.slots):
            raise IndexError("no exposure at %s" % index)
        self.slots[index:index] = [""] * count
        self.active = index
        return count

    def clear_cells(self, start, count=1):
        """Empty the cells and leave the following rows where they are."""
        start, count = self._span(start, count)
        for offset in range(count):
            self.slots[start + offset] = ""
        self.active = start
        return count

    def remove_cells(self, start, count):
        """Cut with shift. Following cells move up. The column may end empty."""
        start, count = self._span(start, count)
        del self.slots[start:start + count]
        self.active = min(start, len(self.slots) - 1) if self.slots else 0
        return count

    def reverse(self, start, count):
        """Swap the selection end for end, blanks included."""
        start, count = self._span(start, count)
        if count < 2:
            return True
        chunk = self.slots[start:start + count]
        chunk.reverse()
        self.slots[start:start + count] = chunk
        self.active = start
        return True

    def swing(self, start, count):
        """Append the selection reversed, without repeating its last cell."""
        start, count = self._span(start, count)
        if count < 2:
            return True
        chunk = self.slots[start:start + count]
        self.slots[start + count:start + count] = list(reversed(chunk[:-1]))
        self.active = start
        return True

    def step(self, start, count, n):
        """Repeat every cell n times, blanks included. Cells after shift down."""
        n = int(n)
        if n < 1:
            raise ValueError("step needs a count")
        start, count = self._span(start, count)
        if n == 1:
            return True
        expanded = []
        for cell in self.slots[start:start + count]:
            expanded.extend([cell] * n)
        self.slots[start:start + count] = expanded
        self.active = start
        return True

    def each(self, start, count, n):
        """Keep cell 0, n, 2n, and delete the others. Shorter than n is a no-op."""
        n = int(n)
        if n < 1:
            raise ValueError("each needs a count")
        start, count = self._span(start, count)
        if count < n:
            return False
        self.slots[start:start + count] = self.slots[start:start + count:n]
        self.active = start
        return True

    def reset_step(self, start, count):
        """Collapse each run of the same cell, blanks included, to one cell."""
        start, count = self._span(start, count)
        cells = self.slots[start:start + count]
        kept = []
        index = 0
        while index < count:
            kept.append(cells[index])
            index += 1
            while index < count and cells[index] == kept[-1]:
                index += 1
        self.slots[start:start + count] = kept
        self.active = start
        return True

    def increase_step(self, start, count):
        """Each non-blank run grows by one. A blank stays."""
        start, count = self._span(start, count)
        cells = self.slots[start:start + count]
        index = 0
        end = count - 1
        while index <= end:
            cell = cells[index]
            if cell:
                cells.insert(index, cell)
                end += 1
                index += 1
                while index <= end and cells[index] == cell:
                    index += 1
            else:
                index += 1
        self.slots[start:start + count] = cells
        self.active = start
        return True

    def decrease_step(self, start, count):
        """Each non-blank run shrinks by one. A run of one stays."""
        start, count = self._span(start, count)
        cells = self.slots[start:start + count]
        index = 0
        end = count - 1
        while index <= end:
            cell = cells[index]
            if cell:
                index += 1
                removed = False
                while index <= end and cells[index] == cell:
                    if not removed:
                        removed = True
                        del cells[index]
                        end -= 1
                    else:
                        index += 1
            else:
                index += 1
        self.slots[start:start + count] = cells
        self.active = start
        return True

    def reframe(self, start, count, step, with_blank=-1):
        """Expose each distinct run for `step` frames.

        A trailing hold of the last selected cell is part of the run.
        with_blank below 0 keeps a blank run. Zero and above drop blanks.
        A positive with_blank then inserts that many blank steps after each drawing.
        """
        step = int(step)
        if step < 1:
            raise ValueError("reframe needs a step")
        start, count = self._span(start, count)
        end = start + count - 1
        last = self.slots[end]
        while end + 1 < len(self.slots) and self.slots[end + 1] == last:
            end += 1
        count = end - start + 1
        unique = []
        for cell in self.slots[start:start + count]:
            if not unique or unique[-1] != cell:
                unique.append(cell)
        if with_blank >= 0:
            unique = [cell for cell in unique if cell]
        if not unique:
            return 0
        gap = int(with_blank) * step if with_blank > 0 else 0
        out = []
        for cell in unique:
            out.extend([cell] * step)
            if gap:
                out.extend([""] * gap)
        self.slots[start:start + count] = out
        self.active = start
        return len(out)

    def autoexpose(self, start, count):
        """Fill or trim so each drawing's exposure matches the number gap.

        Drawing 2 then 5 becomes 2, 2, 2, 5. A decreasing number does nothing.
        A blank stops the fill. The cell just after a trailing hold counts.
        """
        start, count = self._span(start, count)
        r1 = start + count - 1
        index = start
        limit = r1 - 1
        while index <= limit and self.slots[index] == "":
            index += 1
        while index <= limit:
            if index + 1 >= len(self.slots) or self.slots[index + 1] == "":
                break
            if _drawing_number(self.slots[index + 1]) < _drawing_number(self.slots[index]):
                return False
            index += 1
        index = start
        while index <= r1 - 1 and self.slots[index] == "":
            index += 1
        while index <= r1 - 1:
            run = 1
            if index + 1 >= len(self.slots) or self.slots[index + 1] == "":
                index += 1
                continue
            frame1 = _drawing_number(self.slots[index])
            if frame1 < 0:
                break
            while (
                index + 1 < len(self.slots)
                and self.slots[index + 1] != ""
                and _drawing_number(self.slots[index + 1]) == _drawing_number(self.slots[index])
            ):
                index += 1
                run += 1
            if index + 1 >= len(self.slots) or self.slots[index + 1] == "":
                break
            frame2 = _drawing_number(self.slots[index + 1])
            if frame2 < 0:
                break
            if frame1 + run == frame2:
                index += 1
                continue
            if frame1 + run < frame2:
                extra = frame2 - frame1 - run
                cell = self.slots[index]
                for _step in range(extra):
                    self.slots.insert(index + 1, cell)
                index += extra
                r1 += extra
                index += 1
            else:
                extra = run - frame2 + frame1
                index = index - extra
                del self.slots[index + 1:index + 1 + extra]
                r1 -= extra
                index += 1
        self.active = start if self.slots else 0
        return True

    def repeat(self, start, count, times):
        """Append `times` more cycles of the selection. One cycle is the first repeat."""
        times = int(times)
        start, count = self._span(start, count)
        if times < 1:
            return False
        chunk = self.slots[start:start + count]
        self.slots[start + count:start + count] = chunk * times
        self.active = start
        return True

    def random_cells(self, start, count, seed):
        """Shuffle the selection with an explicit seed. The pool stays put."""
        if isinstance(seed, bool) or not isinstance(seed, int):
            raise ValueError("random needs a seed")
        start, count = self._span(start, count)
        order = list(range(count))
        random.Random(seed).shuffle(order)
        chunk = self.slots[start:start + count]
        self.slots[start:start + count] = [chunk[index] for index in order]
        self.active = start
        return True

    def to_row(self):
        """Slots, pool, issued, base_seed, and active. The lists are copies."""
        return {
            "slots": list(self.slots),
            "pool": list(self.pool),
            "issued": self._issued,
            "base_seed": self.base_seed,
            "active": self.active,
        }

    @classmethod
    def from_row(cls, row):
        """Rebuild a sheet from that row. The row's ids are copied, not allocated.

        A bad id, a slot that is not in the pool, or an issued count below the
        pool length raises ValueError. Gaps in the pool stay empty.
        """
        if not isinstance(row, dict):
            raise ValueError("exposure row is missing")
        slots = _cell_list(row.get("slots"))
        pool = _id_list(row.get("pool"))
        issued = _plain_int(row.get("issued"), "issued count is behind the pool")
        if issued < len(pool):
            raise ValueError("issued count is behind the pool")
        known = set(pool)
        for ident in slots:
            if ident and ident not in known:
                raise ValueError("unknown pool frame")
        base_seed = _plain_int(row.get("base_seed"), "base seed is wrong")
        active = _plain_int(row.get("active"), "active slot is wrong")
        if slots:
            if active < 0 or active >= len(slots):
                raise ValueError("active slot is wrong")
        elif active != 0:
            raise ValueError("active slot is wrong")
        sheet = cls(base_seed)
        sheet.slots = slots
        sheet.pool = pool
        sheet._issued = issued
        sheet.active = active
        return sheet


def cel_mark(sheet, index):
    """Hold, reuse, or nothing for one filmstrip cel. No sheet is a no-op."""
    if sheet is None:
        return ""
    state = getattr(sheet, "slot_state", None)
    if not callable(state):
        return ""
    try:
        mark = state(index)
    except (IndexError, TypeError, ValueError):
        return ""
    if mark == "hold" or mark == "reuse":
        return mark
    return ""


def project_slots(pairs):
    """Timeline keys as an X-sheet column. Nothing is saved.

    Rows before the first key are empty. A cel holds until the next key.
    Distinct cels become 000, 001, and so on, in the order they first appear.
    """
    by_frame = {}
    for frame, cel in pairs or []:
        try:
            number = int(frame)
        except (TypeError, ValueError):
            continue
        if number < 0:
            continue
        ident = getattr(cel, "id", None)
        if not ident:
            ident = "" if cel is None else str(cel)
        by_frame[number] = ident
    if not by_frame:
        return []
    ids = {}
    slots = []
    current = ""
    started = False
    for frame in range(max(by_frame) + 1):
        if frame in by_frame:
            current = by_frame[frame]
            started = True
        if not started or not current:
            slots.append("")
            continue
        if current not in ids:
            ids[current] = "%03X" % len(ids)
        slots.append(ids[current])
    return slots


def sheet_from_slots(slots, base_seed=0):
    """A sheet whose slots are these cells. Pool ids are the ones already named."""
    sheet = ExposureSheet(base_seed)
    pool = []
    clean = []
    for slot in slots or []:
        if slot == "" or slot is None:
            clean.append("")
            continue
        ident = _frame_id(slot)
        clean.append(ident)
        if ident not in pool:
            pool.append(ident)
    sheet.slots = clean
    sheet.pool = pool
    sheet._issued = len(pool)
    sheet.active = 0
    return sheet
