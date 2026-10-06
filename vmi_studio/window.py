"""The desk. Layers, the visible stack, and one object per thing."""

import faulthandler
import math
import os
import shutil
import sys
import time
import traceback

from PySide6.QtCore import QEvent, QPointF, QRect, QSize, Qt, QThread, QTimer, Signal
from PySide6.QtGui import (
    QAction,
    QActionGroup,
    QColor,
    QEventPoint,
    QFont,
    QImage,
    QKeySequence,
    QPainter,
    QPen,
    QPixmap,
    QPointingDevice,
    QPolygonF,
)
from PySide6.QtWidgets import (
    QApplication,
    QAbstractItemView,
    QCheckBox,
    QComboBox,
    QDialog,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMainWindow,
    QMenu,
    QMessageBox,
    QPinchGesture,
    QPlainTextEdit,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QSpinBox,
    QSplitter,
    QStyle,
    QStyledItemDelegate,
    QToolTip,
    QTreeWidget,
    QTreeWidgetItem,
    QTreeView,
    QVBoxLayout,
    QWidget,
    QWidgetAction,
)
from PySide6.QtCore import QAbstractItemModel, QModelIndex

from vmi_studio.cut import cut_folder, erase_folder, next_cut_name, parent_of, rect_points
from vmi_studio.history import FingerChords, History, capture
from vmi_studio.composite import (
    BLEND_CHOICES,
    assign_blend,
    blend_label,
    composite_scene,
    ink_count,
    loaded_paint,
    pick_layer,
    present,
    reset_plate_cache,
)
from vmi_studio import fonts
from vmi_studio.navigate import (
    ROTATE_PER_PIXEL,
    ZOOM_STEP,
    clamp_zoom,
    format_zoom,
    gesture,
    view_to_image,
    wheel_factor,
)
from vmi_studio.desk import (
    SOUND_EFFECTS,
    DeskObject,
    apply_frames,
    arrange_objects,
    assigned_type,
    clean_kind,
    clean_playlist,
    clean_sound,
    clean_warp_layer,
    extra_frames,
    _next_object_id,
    frames_of,
    object_z,
    reset_object_ids,
    slot_rows,
    slots_from_subfolders,
    slots_of,
    static_asset_folder,
    static_layers,
)
from vmi_studio.document import (
    animation_folders,
    apply_animations,
    apply_blends,
    apply_track_flags,
    apply_visibility,
    blend_rows,
    arrange,
    can_arrange,
    capture_flags,
    cel_of,
    clamp_opacity,
    find_node,
    move_cel,
    move_into,
    new_folder,
    owning_animation,
    paint_layers,
    panel_order,
    preview_paint,
    rename_node,
    apply_clips,
    set_animation,
    set_clip,
    set_mute,
    set_solo,
    set_visible,
    timeline_cels,
    track_ids,
    visibility_map,
    walk,
)
from vmi_studio.naming import parse_sf, slot_key, state_label
from vmi_studio.openers import open_drawing
from vmi_studio.sceneio import _safe_folder, plan_scene, write_scene
from vmi_studio.vmib import prepare_blueprint, write_parts
from vmi_studio.warp import (
    WARP_PINK,
    WARP_PURPLE,
    mark_warps,
    object_role,
    warp_counts,
    warp_layer_kind,
    warp_system_folder,
    warp_system_layers,
)
from vmi_studio.session import load, save


def range_ids(ordered, anchor, target):
    """Ids from anchor through target, in the order the rows are shown."""
    if target not in ordered:
        return []
    if not anchor or anchor not in ordered:
        return [target]
    start = ordered.index(anchor)
    end = ordered.index(target)
    lo, hi = (start, end) if start <= end else (end, start)
    return ordered[lo:hi + 1]
GUIDE = 16
MARK = 11
KNOB = 18
TREE_WIDTH = 280
MIN_TREE = 160
MIN_TRACK = 200
GUTTER = 7
# Highlight is white. Everything else stays dark.
INK = "#ffffff"
QUIET = "#3a3a3a"
RULE = "#2a2a2a"
PICTURE_WAIT = "Loading the picture."
REST_WAIT = "Loading the rest of the picture."
STATIC_EXPORT = "create new object"
SUBFOLDER_OBJECT = "create new object and generate slot positions from subfolders"
STATIC_GREEN = (
    "QPushButton#staticExport { background: #146b32; color: #e7ffe9; border: 1px solid #3dce73;"
    " padding: 6px 10px; border-radius: 0; text-align: left; }"
    "QPushButton#staticExport:hover { background: #1f8f42; color: #ffffff; border-color: #7dff9a; }"
)
SUBFOLDER_GREEN = (
    "QPushButton#slotExport { background: #0c3a1c; color: #9ddec0; border: 1px solid #1d6b3a;"
    " padding: 6px 10px; border-radius: 0; text-align: left; }"
    "QPushButton#slotExport:hover { background: #145c2e; color: #ffffff; border-color: #3dce73; }"
)
WARP_PREPARE = "prepare as warp target object"
WARP_BUTTON = (
    "QPushButton#warpPrepare { background: #5a2438; color: #f0d0dc; border: 1px solid #a85a78;"
    " padding: 6px 10px; border-radius: 0; text-align: left; }"
    "QPushButton#warpPrepare:hover { background: #7a3450; color: #ffffff; border-color: #e0a0b8; }"
)
# 4K client: narrow side panes, picture takes the rest. A saved layout from
# before this ratio is ignored (split_version).
WIDE_SCREEN = 3200
WIDE_SPLIT = (0.09, 0.82, 0.09)
SPLIT_VERSION = 3
PANE_MIN = 140


def screen_pixels(screen):
    """Panel width in device pixels. This 4K screen is 1920 logical at scale 2."""
    if screen is None:
        return 0
    ratio = screen.devicePixelRatio() or 1
    return int(screen.geometry().width() * ratio)


def split_sizes(width, handle=GUTTER, ratios=WIDE_SPLIT, minimum=PANE_MIN):
    """Pane widths for a splitter of `width` pixels. Handles sit between panes."""
    count = len(ratios)
    inner = int(width) - handle * (count - 1)
    if inner < minimum * count:
        return None
    sizes = [int(round(inner * ratio)) for ratio in ratios]
    sizes[1] += inner - sum(sizes)
    if min(sizes) < minimum:
        return None
    return sizes


def saved_split_fits(saved, width, handle=GUTTER, slack=0.20):
    """True when a saved layout was made at about this splitter width."""
    if not isinstance(saved, (list, tuple)) or len(saved) != 3:
        return False
    try:
        panes = [int(value) for value in saved]
    except (TypeError, ValueError):
        return False
    if any(value < 0 for value in panes):
        return False
    inner = int(width) - handle * 2
    total = sum(panes)
    if inner <= 0 or total <= 0:
        return False
    return abs(total - inner) <= inner * slack


def choose_split(width, saved, screen_width, custom=False):
    """Sizes to apply, or None to leave the gutters alone.

    A layout saved at this width is kept. On a 4K screen, anything saved
    from a much narrower window is replaced by the wide default.
    """
    if custom:
        return None
    if saved_split_fits(saved, width):
        return [int(value) for value in saved]
    if int(screen_width) < WIDE_SCREEN:
        return None
    return split_sizes(width)


def row_marks(rect, depth, kind):
    """Disclosure (folders only), the eye, and where the name starts.

    The two squares do not touch. One opens the folder. One hides the row.
    """
    x0 = rect.x() + 4
    branch = x0 + depth * GUIDE + GUIDE // 2
    top = rect.center().y() - MARK // 2
    if kind == "group":
        disclosure = QRect(branch - MARK // 2, top, MARK, MARK)
    else:
        disclosure = QRect()
    eye = QRect(branch + GUIDE - MARK // 2, top, MARK, MARK)
    return disclosure, eye, eye.right() + 8


def name_rule(name_x, text_width, clip_right, y, row_right):
    """Horizontal rule from the visible end of a name to the right edge of the row.

    Deeper rows and longer names start further right, so the rules stagger.
    """
    visible = min(int(name_x) + int(text_width), int(clip_right))
    start = visible + 8
    end = int(row_right)
    if start >= end:
        return None
    return start, int(y), end, int(y)


def object_row_marks(rect, depth, folder):
    """Same guide column as the layer tree. Objects have no eye."""
    disclosure, _eye, name_x = row_marks(rect, depth, "group" if folder else "layer")
    branch = rect.x() + 4 + depth * GUIDE + GUIDE // 2
    return branch, disclosure, name_x


def _item_depth(item):
    depth = 0
    parent = item.parent()
    while parent is not None:
        depth += 1
        parent = parent.parent()
    return depth


def _item_has_next(item):
    parent = item.parent()
    if parent is None:
        tree = item.treeWidget()
        if tree is None:
            return False
        index = tree.indexOfTopLevelItem(item)
        return 0 <= index < tree.topLevelItemCount() - 1
    index = parent.indexOfChild(item)
    return 0 <= index < parent.childCount() - 1


def _item_chain(item):
    chain = []
    current = item
    while current is not None:
        chain.append(current)
        current = current.parent()
    chain.reverse()
    return chain


def row_tracks(rect, tree_width):
    """The drag zone, mute, solo, clipping mask, opacity knob, and blend menu.

    The drag zone is not drawn. The blend menu uses the rest of the track.
    """
    if not rect.isValid():
        empty = QRect()
        return 0, empty, empty, empty, empty, empty, empty, empty
    split = rect.x() + int(tree_width)
    split = min(max(split, rect.x() + MIN_TREE), rect.right() - MIN_TRACK)
    gutter = QRect(split, rect.y(), GUTTER, rect.height())
    track = QRect(gutter.right() + 1, rect.y(), max(0, rect.right() - gutter.right()), rect.height())
    top = rect.center().y() - MARK // 2
    mute = QRect(track.x() + 6, top, MARK, MARK)
    solo = QRect(mute.right() + 6, top, MARK, MARK)
    clip = QRect(solo.right() + 6, top, MARK, MARK)
    knob_top = rect.center().y() - KNOB // 2
    opacity = QRect(clip.right() + 6, knob_top, KNOB, KNOB)
    blend = QRect(opacity.right() + 8, rect.y() + 2, max(0, track.right() - opacity.right() - 12), max(0, rect.height() - 4))
    return split, gutter, track, mute, solo, clip, opacity, blend


def _paint_handle(painter, box, letter, active):
    if not box.isValid() or box.width() <= 0:
        return
    color = QColor(INK if active else QUIET)
    painter.setPen(QPen(color))
    painter.setBrush(QColor(INK) if active else Qt.NoBrush)
    painter.drawRect(box)
    painter.setPen(QColor("#000000") if active else color)
    font = QFont(painter.font())
    font.setPixelSize(9)
    painter.setFont(font)
    painter.drawText(box, Qt.AlignCenter, letter)


def _paint_clip(painter, box, on, lit):
    """White only on the highlighted row. An unfocused mask stays grey."""
    if not box.isValid() or box.width() <= 0:
        return
    if on and lit:
        frame = QColor(INK)
        fill = QColor(INK)
        letter = QColor("#000000")
    elif on:
        frame = QColor(QUIET)
        fill = QColor(QUIET)
        letter = QColor("#000000")
    else:
        frame = QColor(QUIET)
        fill = None
        letter = frame
    painter.setPen(QPen(frame))
    painter.setBrush(fill if fill is not None else Qt.NoBrush)
    painter.drawRect(box)
    painter.setPen(letter)
    font = QFont(painter.font())
    font.setPixelSize(9)
    painter.setFont(font)
    painter.drawText(box, Qt.AlignCenter, "C")


def _dial_angle(amount):
    """7 o'clock at 0, the long way around to 5 o'clock at 1. Same sweep as Thrash Tracker."""
    span = max(0.0, min(1.0, float(amount)))
    return (2.0 * math.pi / 3.0) + span * (5.0 * math.pi / 3.0)


def _paint_dial(painter, box, amount, text, lit):
    """A rotary. The bump rides the rim. Two hex digits sit in the middle."""
    if not box.isValid() or box.width() < 8:
        return
    color = QColor(INK if lit else QUIET)
    painter.setPen(QPen(color))
    painter.setBrush(Qt.NoBrush)
    painter.drawEllipse(box.adjusted(1, 1, -2, -2))
    angle = _dial_angle(amount)
    center = box.center()
    radius = min(box.width(), box.height()) / 2.0 - 1.0
    bump_x = int(round(center.x() + math.cos(angle) * radius))
    bump_y = int(round(center.y() + math.sin(angle) * radius))
    painter.setBrush(color)
    painter.drawRect(bump_x - 1, bump_y - 1, 3, 2)
    font = QFont(painter.font())
    font.setPixelSize(7)
    painter.setFont(font)
    painter.setPen(color)
    painter.drawText(box, Qt.AlignCenter, text)


def _paint_blend(painter, box, node, lit, font):
    """A short menu box. The mode name is white only on the chosen row."""
    if not box.isValid() or box.width() <= 8:
        return
    painter.setFont(font)
    painter.setPen(QPen(QColor(RULE)))
    painter.setBrush(QColor("#000000"))
    painter.drawRect(box)
    label = blend_label(node.blend, getattr(node, "shapes", True))
    painter.setPen(QColor(INK if lit else QUIET))
    painter.drawText(
        box.adjusted(4, 0, -14, 0),
        Qt.AlignVCenter | Qt.AlignLeft,
        painter.fontMetrics().elidedText(label, Qt.ElideRight, max(0, box.width() - 18)),
    )
    painter.drawText(box.adjusted(0, 0, -3, 0), Qt.AlignVCenter | Qt.AlignRight, "v")


def signature(layers):
    parts = []

    def visit(lst):
        for node in lst:
            parts.append("%s:%s" % (node.kind, node.name))
            visit(node.children)

    visit(layers)
    return "|".join(parts)


def visual_ids(layers, ids):
    rank = {}
    count = {"n": 0}

    def visit(lst):
        for node in panel_order(lst):
            rank[node.id] = count["n"]
            count["n"] += 1
            visit(node.children)

    visit(layers)
    return sorted(ids, key=lambda ident: rank.get(ident, 0))


class LoadThread(QThread):
    ok = Signal(object)
    bad = Signal(str)
    note = Signal(str)

    def __init__(self, path):
        super().__init__()
        self.path = path

    def run(self):
        try:
            art = open_drawing(self.path, progress=lambda text: self.note.emit(text))
        except Exception as exc:
            self.bad.emit(str(exc))
            return
        self.ok.emit(art)


class SaveThread(QThread):
    """Zip the blueprint off the interface thread. A radio-sized file would freeze it."""

    ok = Signal(str)
    bad = Signal(str)

    def __init__(self, path, document, blobs):
        super().__init__()
        self.path = path
        self.document = document
        self.blobs = blobs

    def run(self):
        try:
            write_parts(self.path, self.document, self.blobs)
        except Exception as exc:
            self.bad.emit(str(exc))
            return
        self.ok.emit(self.path)


class LayerModel(QAbstractItemModel):
    def __init__(self):
        super().__init__()
        self.layers = []
        self.picked = set()
        self._parents = {}
        self._nodes = {}
        self._ids = {}
        self._seq = 0

    def set_layers(self, layers):
        mark_warps(layers or [])
        self.beginResetModel()
        self.layers = layers
        self._parents = {}
        self._nodes = {}
        self._ids = {}
        self._seq = 0
        self._index_parents(layers, None)
        self.endResetModel()

    def _index_parents(self, lst, parent):
        for node in lst:
            self._parents[id(node)] = parent
            self._index_parents(node.children, node)

    def _bind(self, node):
        key = self._ids.get(id(node))
        if key is None:
            self._seq += 1
            key = self._seq
            self._ids[id(node)] = key
            self._nodes[key] = node
        return key

    def node_of(self, index):
        if not index.isValid():
            return None
        return self._nodes.get(int(index.internalId()))

    def _children(self, node):
        raw = self.layers if node is None else node.children
        return list(reversed(raw))

    def rowCount(self, parent=QModelIndex()):
        if parent.isValid() and parent.column() != 0:
            return 0
        node = self.node_of(parent)
        return len(node.children if node else self.layers)

    def columnCount(self, parent=QModelIndex()):
        return 1

    def index(self, row, column, parent=QModelIndex()):
        if column != 0 or row < 0:
            return QModelIndex()
        children = self._children(self.node_of(parent))
        if row >= len(children):
            return QModelIndex()
        return self.createIndex(row, 0, self._bind(children[row]))

    def parent(self, index):
        node = self.node_of(index)
        if node is None:
            return QModelIndex()
        parent = self._parents.get(id(node))
        if parent is None:
            return QModelIndex()
        siblings = self._children(self._parents.get(id(parent)))
        try:
            row = siblings.index(parent)
        except ValueError:
            return QModelIndex()
        return self.createIndex(row, 0, self._bind(parent))

    def data(self, index, role=Qt.DisplayRole):
        node = self.node_of(index)
        if node is None:
            return None
        if role in (Qt.DisplayRole, Qt.EditRole):
            return node.name
        if role == Qt.UserRole:
            return node.id
        return None

    def flags(self, index):
        if not index.isValid():
            return Qt.NoItemFlags
        return Qt.ItemIsEnabled | Qt.ItemIsSelectable

    def ancestors(self, node):
        chain = []
        parent = self._parents.get(id(node))
        while parent is not None:
            chain.append(parent)
            parent = self._parents.get(id(parent))
        chain.reverse()
        return chain

    def depth(self, node):
        return len(self.ancestors(node))

    def has_next(self, node):
        siblings = self._children(self._parents.get(id(node)))
        try:
            return siblings.index(node) < len(siblings) - 1
        except ValueError:
            return False

    def guide_continues(self, node, level):
        chain = self.ancestors(node)
        if level < 0 or level >= len(chain):
            return False
        return self.has_next(chain[level])

    def index_for(self, node, parent=QModelIndex()):
        children = self._children(self.node_of(parent))
        for row, child in enumerate(children):
            index = self.index(row, 0, parent)
            if child is node:
                return index
            found = self.index_for(node, index)
            if found.isValid():
                return found
        return QModelIndex()


def _outliner(widget):
    if isinstance(widget, Outliner):
        return widget
    parent = widget.parent() if widget is not None else None
    if isinstance(parent, Outliner):
        return parent
    return None


class LayerDelegate(QStyledItemDelegate):
    def sizeHint(self, option, index):
        return QSize(option.rect.width(), 22)

    def paint(self, painter, option, index):
        model = index.model()
        node = model.node_of(index)
        if node is None:
            return
        painter.save()
        painter.setRenderHint(QPainter.Antialiasing, False)
        rect = option.rect
        selected = bool(option.state & QStyle.StateFlag.State_Selected) or node.id in model.picked
        lit = selected or bool(option.state & QStyle.StateFlag.State_HasFocus)
        view = _outliner(option.widget)
        tree_width = view.tree_width if view is not None else TREE_WIDTH
        split, _gutter, _track, mute, solo, clip, opacity, blend = row_tracks(rect, tree_width)
        painter.fillRect(rect, QColor("#111111" if selected else "#000000"))
        painter.setClipRect(rect.x(), rect.y(), max(0, split - rect.x()), rect.height())
        pen = QPen(QColor(RULE))
        pen.setWidth(1)
        painter.setPen(pen)
        painter.setBrush(Qt.NoBrush)
        depth = model.depth(node)
        x0 = rect.x() + 4
        mid = rect.center().y()
        for level in range(depth):
            if not model.guide_continues(node, level):
                continue
            gx = x0 + level * GUIDE + GUIDE // 2
            painter.drawLine(gx, rect.top(), gx, rect.bottom())
        branch = x0 + depth * GUIDE + GUIDE // 2
        disclosure, eye, name_x = row_marks(rect, depth, node.kind)
        painter.drawLine(branch, rect.top(), branch, mid)
        if model.has_next(node):
            painter.drawLine(branch, mid, branch, rect.bottom())
        if disclosure.isValid():
            painter.drawLine(branch, mid, disclosure.left(), mid)
        else:
            painter.drawLine(branch, mid, eye.left(), mid)
        mark = QColor(INK if selected else QUIET)
        painter.setPen(mark)
        painter.setBrush(Qt.NoBrush)
        if disclosure.isValid():
            painter.drawRect(disclosure)
            painter.drawLine(disclosure.left() + 2, disclosure.center().y(), disclosure.right() - 2, disclosure.center().y())
            opened = view is not None and view.isExpanded(index)
            if not opened:
                painter.drawLine(disclosure.center().x(), disclosure.top() + 2, disclosure.center().x(), disclosure.bottom() - 2)
        painter.setBrush(mark if node.visible else Qt.NoBrush)
        painter.drawRect(eye)
        if selected:
            painter.setPen(QColor(INK))
        elif getattr(node, "warp", "") == "target":
            painter.setPen(QColor(WARP_PINK))
        elif getattr(node, "warp", "") == "window":
            painter.setPen(QColor(WARP_PURPLE))
        elif node.omit:
            painter.setPen(QColor(RULE))
        else:
            painter.setPen(QColor(QUIET))
        text = node.name if not node.omit else "%s  %s" % (node.name, node.omit)
        painter.setFont(option.font)
        reserve = 36 if node.animation else 6
        painter.drawText(
            name_x, rect.top(), max(0, split - name_x - reserve), rect.height(),
            Qt.AlignVCenter, text,
        )
        if node.animation:
            painter.setPen(QColor(INK if selected else QUIET))
            painter.drawText(
                split - 34, rect.top(), 30, rect.height(),
                Qt.AlignRight | Qt.AlignVCenter, "ANI",
            )
        text_width = painter.fontMetrics().horizontalAdvance(text)
        painter.setClipping(False)
        rule = name_rule(name_x, text_width, split - reserve, rect.bottom(), rect.right())
        if rule is not None:
            painter.setPen(QPen(QColor(RULE)))
            painter.drawLine(rule[0], rule[1], rule[2], rule[3])
        _paint_handle(painter, mute, "M", bool(node.mute))
        _paint_handle(painter, solo, "S", bool(node.solo))
        _paint_clip(painter, clip, bool(node.clip), lit)
        level = clamp_opacity(getattr(node, "opacity", 255))
        _paint_dial(painter, opacity, level / 255.0, "%02X" % level, lit)
        _paint_blend(painter, blend, node, lit, option.font)
        painter.restore()


class Outliner(QTreeView):
    def __init__(self, model, host):
        super().__init__()
        self.setModel(model)
        self.host = host
        self._editor = None
        self._anchor = None
        self.setItemDelegate(LayerDelegate(self))
        self.setHeaderHidden(True)
        self.setIndentation(0)
        self.setRootIsDecorated(False)
        self.setExpandsOnDoubleClick(False)
        self.setAnimated(False)
        self.setUniformRowHeights(True)
        self.setContextMenuPolicy(Qt.CustomContextMenu)
        self.customContextMenuRequested.connect(self._menu)
        self.setFrameShape(QFrame.NoFrame)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.viewport().setMouseTracking(True)
        self.tree_width = TREE_WIDTH
        self._press = None
        self._press_id = None
        self._press_keep = False
        self._dragging = False
        self._drop = None
        self._refused = False
        self._gutter_drag = False
        self._opacity_id = None
        model.modelReset.connect(self.expandAll)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        width = self.viewport().width()
        if width > 0 and self.columnWidth(0) != width:
            self.setColumnWidth(0, width)

    def track_marks(self, index):
        rect = self.visualRect(index)
        return row_tracks(rect, self.tree_width)

    def row_marks(self, index):
        node = self.model().node_of(index)
        rect = self.visualRect(index)
        if node is None or not rect.isValid():
            return QRect(), QRect(), 0
        return row_marks(rect, self.model().depth(node), node.kind)

    def visible_nodes(self):
        """Rows on screen, top to bottom. A closed folder hides its children."""
        found = []

        def walk(parent):
            model = self.model()
            for row in range(model.rowCount(parent)):
                index = model.index(row, 0, parent)
                node = model.node_of(index)
                if node is None:
                    continue
                found.append(node)
                if node.kind == "group" and self.isExpanded(index):
                    walk(index)

        walk(QModelIndex())
        return found

    def _zone(self, index, pos):
        node = self.model().node_of(index)
        if node is None:
            return "row"
        _split, gutter, track, mute, solo, clip, opacity, blend = self.track_marks(index)
        if gutter.contains(pos):
            return "gutter"
        if mute.contains(pos):
            return "mute"
        if solo.contains(pos):
            return "solo"
        if clip.contains(pos):
            return "clip"
        if opacity.isValid() and opacity.contains(pos):
            return "opacity"
        if blend.isValid() and blend.width() > 8 and blend.contains(pos):
            return "blend"
        if track.isValid() and pos.x() >= track.left():
            return "track"
        disclosure, eye, name_x = self.row_marks(index)
        if disclosure.isValid() and disclosure.contains(pos):
            return "branch"
        if eye.contains(pos):
            return "eye"
        if pos.x() >= name_x:
            return "name"
        return "row"

    def mousePressEvent(self, event):
        self._end_drag()
        index = self.indexAt(event.position().toPoint())
        if not index.isValid() or event.button() != Qt.LeftButton:
            super().mousePressEvent(event)
            return
        pos = event.position().toPoint()
        zone = self._zone(index, pos)
        node = self.model().node_of(index)
        if node is None:
            super().mousePressEvent(event)
            return
        if zone == "branch":
            self.setExpanded(index, not self.isExpanded(index))
            opened = self.isExpanded(index)
            self.host.status.showMessage("%s %s." % ("Opened" if opened else "Closed", node.name))
            return
        if zone == "eye":
            self.host.toggle_visible(node)
            return
        if zone == "mute":
            self.host.toggle_mute(node)
            return
        if zone == "solo":
            self.host.toggle_solo(node)
            return
        if zone == "clip":
            self.host.toggle_clip(node)
            return
        if zone == "opacity":
            self._opacity_id = node.id
            self.host.begin_opacity(node, pos.y())
            self.viewport().setCursor(Qt.SizeVerCursor)
            return
        if zone == "blend":
            self.host.choose_blend(node, event.globalPosition().toPoint())
            return
        if zone == "gutter":
            self._gutter_drag = True
            self.viewport().setCursor(Qt.SplitHCursor)
            return
        if zone == "track":
            if not self._select_from_row(index, node, event.modifiers()):
                self.host.select_only(node)
            return
        if self._select_from_row(index, node, event.modifiers()):
            return
        self._arm_drag(index, node, pos)

    def _select_from_row(self, index, node, modifiers):
        if modifiers & Qt.ShiftModifier:
            if not self._anchor:
                self._anchor = node.id
                self.host.select_only(node)
            else:
                self.host.select_range(self._anchor, node.id)
            return True
        if modifiers & Qt.ControlModifier:
            self.setCurrentIndex(index)
            self.host.toggle_pick(node)
            return True
        return False

    def _arm_drag(self, index, node, pos):
        picked = self.model().picked
        self._press = pos
        self._press_id = node.id
        self._press_keep = node.id in picked and len(picked) > 1 and not self.host.arrange_locked
        if self._press_keep:
            self.setCurrentIndex(index)
            return
        self.host.select_only(node)

    def mouseMoveEvent(self, event):
        pos = event.position().toPoint()
        if self._opacity_id is not None and event.buttons() & Qt.LeftButton:
            self.viewport().setCursor(Qt.SizeVerCursor)
            self.host.drag_opacity(pos.y())
            return
        if self._gutter_drag and event.buttons() & Qt.LeftButton:
            limit = self.viewport().width() - MIN_TRACK
            self.tree_width = max(MIN_TREE, min(pos.x(), limit))
            self.viewport().setCursor(Qt.SplitHCursor)
            self.viewport().update()
            return
        if self._press is not None and event.buttons() & Qt.LeftButton:
            pos = event.position().toPoint()
            delta = pos - self._press
            if abs(delta.x()) + abs(delta.y()) >= 4:
                if self.host.arrange_locked or self._editor is not None:
                    if not self._refused:
                        self._refused = True
                        self.host.status.showMessage("Unlock the layers to rearrange them.")
                    return
                self._dragging = True
                self._update_drop(pos)
                return
        index = self.indexAt(event.position().toPoint())
        tip = ""
        zone = ""
        if index.isValid():
            zone = self._zone(index, event.position().toPoint())
            node = self.model().node_of(index)
            if zone == "branch":
                tip = "Close folder" if self.isExpanded(index) else "Open folder"
            elif zone == "eye" and node is not None:
                tip = "Hide" if node.visible else "Show"
            elif zone == "mute" and node is not None:
                tip = "Unmute" if node.mute else "Mute"
            elif zone == "solo" and node is not None:
                tip = "Solo off" if node.solo else "Solo"
            elif zone == "clip" and node is not None:
                tip = "Clipping mask on" if node.clip else "Clipping mask off"
            elif zone == "opacity" and node is not None:
                tip = "Opacity %02X. Drag up to raise it." % clamp_opacity(node.opacity)
                self.viewport().setCursor(Qt.SizeVerCursor)
            elif zone == "blend" and node is not None:
                tip = "Blend mode"
            elif zone == "gutter":
                tip = "Drag to give the tracks more room"
                self.viewport().setCursor(Qt.SplitHCursor)
            elif zone == "track":
                tip = "Track"
        if tip:
            QToolTip.showText(event.globalPosition().toPoint(), tip, self.viewport())
        else:
            QToolTip.hideText()
        if self._opacity_id is None and not self._gutter_drag and zone not in ("opacity", "gutter"):
            self.viewport().unsetCursor()
        super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event):
        if self._opacity_id is not None and event.button() == Qt.LeftButton:
            self._opacity_id = None
            self.viewport().unsetCursor()
            self.host.end_opacity()
            return
        if self._gutter_drag and event.button() == Qt.LeftButton:
            self._gutter_drag = False
            self.viewport().unsetCursor()
            if self.host.art:
                self.host._save_session()
            return
        if event.button() != Qt.LeftButton or self._press is None:
            super().mouseReleaseEvent(event)
            return
        press_id = self._press_id
        keep = self._press_keep
        dragging = self._dragging
        drop = self._drop
        self._end_drag()
        if dragging and drop is not None:
            index, place = drop
            node = self.model().node_of(index)
            if node is not None:
                ids = list(self.model().picked)
                if press_id not in ids:
                    ids = [press_id]
                self.host.arrange_layers(ids, node.id, place)
            return
        if keep:
            node = find_node(self.model().layers, press_id)
            if node is not None:
                self.host.select_only(node)
            return
        super().mouseReleaseEvent(event)

    def paintEvent(self, event):
        super().paintEvent(event)
        if not self._drop:
            return
        index, place = self._drop
        rect = self.visualRect(index)
        if not rect.isValid():
            return
        painter = QPainter(self.viewport())
        painter.setRenderHint(QPainter.Antialiasing, False)
        painter.setPen(QPen(QColor(INK)))
        painter.setBrush(Qt.NoBrush)
        if place == "into":
            painter.drawRect(rect.adjusted(1, 1, -2, -2))
        else:
            y = rect.top() if place == "before" else rect.bottom()
            painter.drawLine(rect.left() + 8, y, rect.right() - 2, y)
        painter.end()

    def _drag_ids(self):
        ids = list(self.model().picked)
        if self._press_id not in ids:
            ids = [self._press_id]
        return ids

    def _update_drop(self, pos):
        index = self.indexAt(pos)
        place = None
        if index.isValid():
            rect = self.visualRect(index)
            node = self.model().node_of(index)
            if node is not None and rect.height() > 0:
                local = pos.y() - rect.top()
                height = rect.height()
                if local < height * 0.25:
                    place = "before"
                elif local > height * 0.75:
                    place = "after"
                elif node.kind == "group":
                    place = "into"
                elif local < height * 0.5:
                    place = "before"
                else:
                    place = "after"
        elif self.model().layers:
            rows = self.visible_nodes()
            if rows:
                last = self.model().index_for(rows[-1])
                rect = self.visualRect(last)
                if last.isValid() and pos.y() >= rect.bottom():
                    index = last
                    place = "after"
        if place is None or not index.isValid():
            self._drop = None
            self.viewport().setCursor(Qt.ForbiddenCursor)
        else:
            node = self.model().node_of(index)
            allowed = node is not None and can_arrange(self.model().layers, self._drag_ids(), node.id, place)
            self._drop = (index, place) if allowed else None
            self.viewport().setCursor(Qt.ClosedHandCursor if allowed else Qt.ForbiddenCursor)
        self.viewport().update()

    def _end_drag(self):
        self._press = None
        self._press_id = None
        self._press_keep = False
        self._dragging = False
        self._drop = None
        self._refused = False
        self.viewport().unsetCursor()
        self.viewport().update()

    def _step(self, up):
        rows = self.visible_nodes()
        if not rows:
            return None
        current = self.model().node_of(self.currentIndex())
        if current not in rows:
            return rows[-1] if up else rows[0]
        index = rows.index(current)
        nxt = index - 1 if up else index + 1
        nxt = max(0, min(len(rows) - 1, nxt))
        return rows[nxt]

    def mouseDoubleClickEvent(self, event):
        index = self.indexAt(event.position().toPoint())
        if index.isValid() and self._zone(index, event.position().toPoint()) == "name":
            self._begin_rename(index)
            return
        event.accept()

    def keyPressEvent(self, event):
        if event.key() in (Qt.Key_Up, Qt.Key_Down):
            node = self._step(event.key() == Qt.Key_Up)
            if node is None:
                return
            if event.modifiers() & Qt.ShiftModifier:
                if not self._anchor:
                    current = self.model().node_of(self.currentIndex())
                    self._anchor = current.id if current is not None else node.id
                self.host.select_range(self._anchor, node.id)
            else:
                self.host.select_only(node)
            return
        index = self.currentIndex()
        if index.isValid() and event.key() == Qt.Key_H:
            self.host.toggle_visible(self.model().node_of(index))
            return
        if index.isValid() and event.key() == Qt.Key_F2:
            self._begin_rename(index)
            return
        super().keyPressEvent(event)

    def _begin_rename(self, index):
        node = self.model().node_of(index)
        if node is None:
            return
        if self._editor is not None:
            self._editor.deleteLater()
            self._editor = None
        rect = self.visualRect(index)
        _disclosure, _eye, name_x = self.row_marks(index)
        edit = QLineEdit(self.viewport())
        edit.setText(node.name)
        edit.setGeometry(name_x, rect.y(), max(80, rect.right() - name_x), rect.height())
        edit.setProperty("nodeId", node.id)
        edit.selectAll()
        edit.show()
        edit.setFocus()
        edit.editingFinished.connect(self._finish_rename)
        self._editor = edit

    def _finish_rename(self):
        edit = self._editor
        if edit is None:
            return
        self._editor = None
        ident = edit.property("nodeId")
        name = edit.text()
        edit.deleteLater()
        self.host.rename_layer(ident, name)

    def _menu_button(self, menu, text, object_name, style):
        button = QPushButton(text, menu)
        button.setObjectName(object_name)
        button.setCursor(Qt.PointingHandCursor)
        button.setStyleSheet(style)
        button.setMinimumHeight(28)
        button.setMinimumWidth(button.fontMetrics().horizontalAdvance(text) + 28)
        action = QWidgetAction(menu)
        action.setDefaultWidget(button)

        def fire():
            menu._chosen = action
            menu.close()

        button.clicked.connect(fire)
        menu.addAction(action)
        return action

    def _fill_menu(self, menu, node):
        """Warp and green folder actions first, then the ordinary layer actions."""
        menu._chosen = None
        warp_prepare = None
        static_export = None
        subfolders = None
        special = False
        if warp_system_folder(node):
            warp_prepare = self._menu_button(menu, WARP_PREPARE, "warpPrepare", WARP_BUTTON)
            special = True
        if static_asset_folder(node):
            static_export = self._menu_button(menu, STATIC_EXPORT, "staticExport", STATIC_GREEN)
            subfolders = self._menu_button(menu, SUBFOLDER_OBJECT, "slotExport", SUBFOLDER_GREEN)
            special = True
        if special:
            menu.addSeparator()
        rename = menu.addAction("Rename")
        hide = menu.addAction("Show" if not node.visible else "Hide")
        grouping = len(self.model().picked) > 1
        folder = menu.addAction("Create folder and insert layer" if grouping else "New folder")
        anim = None
        if node.kind == "group":
            anim = menu.addAction("Not an animation" if node.animation else "Animation")
        move = menu.addAction("Move picked here")
        move.setEnabled(node.kind == "group" and bool(self.model().picked))
        return warp_prepare, static_export, subfolders, rename, hide, folder, anim, move

    def _menu(self, pos):
        index = self.indexAt(pos)
        if not index.isValid():
            return
        self.setCurrentIndex(index)
        node = self.model().node_of(index)
        menu = QMenu(self)
        warp_prepare, static_export, subfolders, rename, hide, folder, anim, move = self._fill_menu(menu, node)
        grouping = len(self.model().picked) > 1
        picked = menu.exec(self.viewport().mapToGlobal(pos))
        chosen = menu._chosen if menu._chosen is not None else picked
        if chosen == warp_prepare and warp_prepare is not None:
            self.host.prepare_warp_object(node)
        elif chosen == static_export and static_export is not None:
            self.host.create_folder_object(node)
        elif chosen == subfolders and subfolders is not None:
            self.host.object_from_subfolders(node)
        elif chosen == rename:
            self._begin_rename(index)
        elif chosen == hide:
            self.host.toggle_visible(node)
        elif chosen == folder:
            if grouping:
                self.host.group_selection()
            else:
                self.host.make_folder(node)
        elif anim is not None and chosen == anim:
            self.host.mark_animation(node)
        elif chosen == move:
            self.host.move_picked(node)


class ArrangeLock(QPushButton):
    """Closed by default. Open (checked) means layers can be dragged."""

    def __init__(self):
        super().__init__()
        self.setCheckable(True)
        self.setFixedSize(22, 22)
        self.setCursor(Qt.PointingHandCursor)
        self.setStyleSheet("QPushButton { padding: 0; border: none; background: #000000; }")
        self._tip()

    def setChecked(self, checked):
        super().setChecked(checked)
        self._tip()

    def _tip(self):
        if self.isChecked():
            self.setToolTip("Arrangement unlocked. Drag layers and folders. Click to lock.")
        else:
            self.setToolTip("Arrangement locked. Click to drag layers and folders.")

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing, False)
        unlocked = self.isChecked()
        color = QColor(INK if unlocked else QUIET)
        edge = QColor(INK if unlocked else RULE)
        painter.fillRect(self.rect(), QColor("#000000"))
        painter.setPen(QPen(edge))
        painter.setBrush(Qt.NoBrush)
        painter.drawRect(0, 0, self.width() - 1, self.height() - 1)
        painter.setPen(QPen(color))
        painter.drawRect(6, 11, 9, 7)
        painter.drawPoint(10, 14)
        painter.drawLine(8, 11, 8, 6)
        painter.drawLine(8, 6, 13, 6)
        if unlocked:
            painter.drawLine(13, 6, 13, 8)
        else:
            painter.drawLine(13, 6, 13, 11)
        painter.end()


class PictureView(QWidget):
    """The picture, with KOOLDRAW's zoom, pan, and rotate.

    Scroll zooms. Space+drag and the middle button pan. Shift+drag rotates
    a mouse. A plain left click picks the layer under the cursor.
    """

    def __init__(self):
        super().__init__()
        self._image = None
        self._message = "Open a drawing."
        self._busy = False
        self._busy_text = PICTURE_WAIT
        self._busy_phase = 0
        self._busy_timer = QTimer(self)
        self._busy_timer.setInterval(40)
        self._busy_timer.timeout.connect(self._busy_tick)
        self._doc = (0, 0)
        self.fit = True
        self.zoom = 1.0
        self.pan_x = 0.0
        self.pan_y = 0.0
        self.rotation = 0.0
        self.on_resize = None
        self.on_pick = None
        self.cut_active = False
        self.cut_tool = "rect"
        self.selection = []
        self._draft = []
        self._hover = None
        self._space = False
        self._drag = None
        self._last = None
        self._press = None
        self._pinch_scale = 1.0
        self._pinch_rotation = 0.0
        self._pinch_center = None
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.ClickFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_AcceptTouchEvents, True)
        self.grabGesture(Qt.GestureType.PinchGesture)
        self._zoom_out = self._zoom_button("−", "Zoom out")
        self._zoom_label = self._zoom_button("1×", "Fit picture")
        self._zoom_label.setFixedSize(48, 24)
        self._zoom_in = self._zoom_button("+", "Zoom in")
        self._zoom_out.clicked.connect(lambda: self.zoom_by(1.0 / ZOOM_STEP))
        self._zoom_in.clicked.connect(lambda: self.zoom_by(ZOOM_STEP))
        self._zoom_label.clicked.connect(lambda: self.set_fit(True))
        app = QApplication.instance()
        if app is not None:
            app.installEventFilter(self)

    def _zoom_button(self, text, tip):
        button = QPushButton(text, self)
        button.setFixedSize(28, 24)
        button.setToolTip(tip)
        button.setStyleSheet(
            "QPushButton { padding: 0; border: 1px solid #2a2a2a; background: #000000; color: #3a3a3a; }"
            "QPushButton:hover { border-color: #ffffff; color: #ffffff; }"
        )
        return button

    def viewport(self):
        return self

    def set_document(self, width, height):
        self._doc = (int(width), int(height))

    def set_image(self, pixmap):
        self._image = pixmap
        self._message = ""
        self.apply()

    def set_fit(self, fit):
        self.fit = bool(fit)
        self.reset_navigation()
        self.apply()
        if self.on_resize:
            self.on_resize()

    def reset_view(self):
        """Fit the next drawing. Does not rebuild the picture by itself."""
        self.fit = True
        self.reset_navigation()
        self.apply()

    def reset_navigation(self):
        self.zoom = 1.0
        self.pan_x = 0.0
        self.pan_y = 0.0
        self.rotation = 0.0

    def apply(self):
        self._sync_zoom_label()
        self.update()

    def clear_message(self, text):
        self._image = None
        self._message = text
        self.apply()

    def is_busy(self):
        return bool(self._busy)

    def set_busy(self, busy, text=None):
        """An indeterminate bar while a new picture replaces the one on screen."""
        if text:
            self._busy_text = text
        self._busy = bool(busy)
        if self._busy:
            if not self._busy_timer.isActive():
                self._busy_timer.start()
        else:
            self._busy_timer.stop()
        self.update()

    def _busy_tick(self):
        self._busy_phase = (self._busy_phase + 1) % 28
        self.update()

    def zoom_by(self, factor):
        base = self._base_scale()
        shown = clamp_zoom(base * self.zoom * float(factor))
        self.zoom = shown / base if base > 0 else 1.0
        self.apply()

    def _base_scale(self):
        image = self._image
        if image is None or image.isNull() or image.width() < 1 or image.height() < 1:
            return 1.0
        if not self.fit:
            return 1.0
        return min(self.width() / image.width(), self.height() / image.height())

    def _shown_scale(self):
        return self._base_scale() * self.zoom

    def _sync_zoom_label(self):
        self._zoom_label.setText("%s×" % format_zoom(self._shown_scale()))

    def image_at(self, pos):
        image = self._image
        if image is None or image.isNull():
            return None
        x = pos.x() if hasattr(pos, "x") else pos[0]
        y = pos.y() if hasattr(pos, "y") else pos[1]
        return view_to_image(
            x, y, self.width(), self.height(),
            image.width(), image.height(),
            self._shown_scale(), self.pan_x, self.pan_y, self.rotation,
        )

    def document_at(self, pos):
        hit = self.image_at(pos)
        if hit is None or self._image is None or self._image.isNull():
            return None
        ix, iy = hit
        iw = self._image.width()
        ih = self._image.height()
        if iw < 1 or ih < 1:
            return None
        dw, dh = self._doc
        if dw <= 0 or dh <= 0:
            dw, dh = iw, ih
        return ix / iw * dw, iy / ih * dh

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._place_zoom()
        self._sync_zoom_label()
        if self.on_resize:
            self.on_resize()

    def _place_zoom(self):
        gap = 4
        top = 8
        right = self.width() - 8
        self._zoom_in.move(right - self._zoom_in.width(), top)
        self._zoom_label.move(self._zoom_in.x() - gap - self._zoom_label.width(), top)
        self._zoom_out.move(self._zoom_label.x() - gap - self._zoom_out.width(), top)
        for button in (self._zoom_out, self._zoom_label, self._zoom_in):
            button.raise_()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), QColor("#000000"))
        painter.setRenderHint(QPainter.Antialiasing, False)
        image = self._image
        if image is None or image.isNull():
            painter.setPen(QColor(QUIET))
            painter.drawText(self.rect(), Qt.AlignCenter | Qt.TextWordWrap, self._message or "Open a drawing.")
        else:
            scale = self._shown_scale()
            painter.translate(self.width() / 2.0 + self.pan_x, self.height() / 2.0 + self.pan_y)
            painter.rotate(self.rotation)
            painter.scale(scale, scale)
            painter.translate(-image.width() / 2.0, -image.height() / 2.0)
            painter.drawPixmap(0, 0, image)
            self._paint_selection(painter)
            painter.resetTransform()
        if self.height() >= 36:
            if self.cut_active:
                text = "SHIFT+X DONE   ·   DRAG A SELECTION   ·   CUT TO FOLDER OR DELETE"
            else:
                text = "SCROLL ZOOM   ·   SPACE PAN   ·   SHIFT ROTATE   ·   CLICK A LAYER"
            painter.setPen(QColor(RULE))
            painter.drawText(8, self.height() - 8, text)
        if self._busy and self.width() > 24 and self.height() > 48:
            bar = QRect(8, self.height() - 28, self.width() - 16, 6)
            painter.fillRect(bar, QColor(RULE))
            span = max(24, bar.width() // 5)
            travel = max(1, bar.width() - span)
            x = bar.left() + (self._busy_phase * travel) // 27
            painter.fillRect(x, bar.top(), span, bar.height(), QColor(INK))
            painter.setPen(QColor(QUIET))
            painter.drawText(8, bar.top() - 6, self._busy_text)
        painter.end()

    def set_cut(self, active):
        self.cut_active = bool(active)
        if not active:
            self.clear_selection()
        self.update()

    def clear_selection(self):
        self.selection = []
        self._draft = []
        self._hover = None
        self.update()

    def close_polygon(self):
        if self.cut_tool != "poly" or len(self._draft) < 3:
            return False
        self.selection = list(self._draft)
        self._draft = []
        self._hover = None
        self.update()
        return True

    def pop_point(self):
        if self.cut_tool != "poly" or not self._draft:
            return
        self._draft.pop()
        self.selection = []
        self.update()

    def _preview_points(self):
        if self.selection and not self._draft:
            return self.selection
        if self.cut_tool == "rect" and len(self._draft) == 2:
            x0, y0 = self._draft[0]
            x1, y1 = self._draft[1]
            return rect_points(x0, y0, x1, y1)
        points = list(self._draft)
        if self.cut_tool == "poly" and self._hover is not None and points:
            points.append(self._hover)
        return points

    def _doc_to_preview(self, x, y):
        image = self._image
        dw, dh = self._doc
        if image is None or image.isNull() or dw <= 0 or dh <= 0:
            return float(x), float(y)
        return float(x) / float(dw) * image.width(), float(y) / float(dh) * image.height()

    def _paint_selection(self, painter):
        if not self.cut_active:
            return
        points = self._preview_points()
        if len(points) < 2:
            return
        mapped = [self._doc_to_preview(x, y) for x, y in points]
        pen = QPen(QColor(INK))
        pen.setCosmetic(True)
        painter.setPen(pen)
        painter.setBrush(Qt.NoBrush)
        polygon = QPolygonF([QPointF(x, y) for x, y in mapped])
        if self.selection and not self._draft:
            painter.drawPolygon(polygon)
        else:
            painter.drawPolyline(polygon)

    def _cut_press(self, event):
        if not self.cut_active or event.button() != Qt.LeftButton or self._space:
            return False
        doc = self.document_at(event.position())
        if doc is None:
            event.accept()
            return True
        if self.cut_tool == "poly":
            self.selection = []
            self._draft.append(doc)
            self.update()
            event.accept()
            return True
        self.selection = []
        self._draft = [doc]
        self._drag = "cut"
        self._press = event.position()
        self.update()
        event.accept()
        return True

    def keyPressEvent(self, event):
        if not self._text_focus() and not (event.modifiers() & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)):
            if event.key() in (Qt.Key_Plus, Qt.Key_Equal):
                self.zoom_by(ZOOM_STEP)
                event.accept()
                return
            if event.key() == Qt.Key_Minus:
                self.zoom_by(1.0 / ZOOM_STEP)
                event.accept()
                return
        super().keyPressEvent(event)

    def wheelEvent(self, event):
        pixel = event.pixelDelta()
        if pixel.y() != 0:
            dy = -float(pixel.y())
        else:
            # One Qt notch is 120. KOOLDRAW treats a mouse notch as about 100 pixels.
            dy = -float(event.angleDelta().y()) * (100.0 / 120.0)
        self.zoom_by(wheel_factor(dy, 0))
        event.accept()

    def mouseDoubleClickEvent(self, event):
        if self.cut_active and self.cut_tool == "poly" and event.button() == Qt.LeftButton:
            if len(self._draft) >= 2:
                last = self._draft[-1]
                prev = self._draft[-2]
                if abs(last[0] - prev[0]) < 2 and abs(last[1] - prev[1]) < 2:
                    self._draft.pop()
            self.close_polygon()
            event.accept()
            return
        super().mouseDoubleClickEvent(event)

    def mousePressEvent(self, event):
        if self._cut_press(event):
            return
        mode = self._gesture(event)
        if mode in ("pan", "rotate"):
            self._drag = mode
            self._last = event.position()
            self.grabMouse()
            self._cursor()
            event.accept()
            return
        if mode == "pick":
            self._drag = "pick"
            self._press = event.position()
            event.accept()
            return
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event):
        if self.cut_active and self.cut_tool == "poly" and self._drag != "cut":
            self._hover = self.document_at(event.position())
            self.update()
        if self._drag == "cut":
            doc = self.document_at(event.position())
            if doc is not None and self._draft:
                if self.cut_tool == "rect":
                    self._draft = [self._draft[0], doc]
                elif not self._draft or abs(doc[0] - self._draft[-1][0]) + abs(doc[1] - self._draft[-1][1]) >= 1.5:
                    self._draft.append(doc)
            self.update()
            event.accept()
            return
        if self._drag == "pan" and self._last is not None:
            pos = event.position()
            self.pan_x += pos.x() - self._last.x()
            self.pan_y += pos.y() - self._last.y()
            self._last = pos
            self.update()
            event.accept()
            return
        if self._drag == "rotate" and self._last is not None:
            pos = event.position()
            self.rotation += (pos.x() - self._last.x()) * ROTATE_PER_PIXEL
            self._last = pos
            self.update()
            event.accept()
            return
        self._cursor()
        super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event):
        drag = self._drag
        press = self._press
        self._drag = None
        self._last = None
        self._press = None
        if self.mouseGrabber() is self:
            self.releaseMouse()
        self._cursor()
        if drag == "cut":
            if self.cut_tool == "rect" and len(self._draft) == 2:
                x0, y0 = self._draft[0]
                x1, y1 = self._draft[1]
                if abs(x1 - x0) >= 1 and abs(y1 - y0) >= 1:
                    self.selection = rect_points(x0, y0, x1, y1)
            elif self.cut_tool == "lasso" and len(self._draft) >= 3:
                self.selection = list(self._draft)
            self._draft = []
            self.update()
            event.accept()
            return
        if drag == "pick" and press is not None and event.button() == Qt.LeftButton:
            moved = (event.position() - press).manhattanLength()
            if moved < 4:
                self._emit_pick(event.position())
            event.accept()
            return
        super().mouseReleaseEvent(event)

    def event(self, event):
        if event.type() == QEvent.Type.Gesture:
            return self._pinch(event)
        return super().event(event)

    def _pinch(self, event):
        pinch = event.gesture(Qt.GestureType.PinchGesture)
        if not isinstance(pinch, QPinchGesture):
            return False
        state = pinch.state()
        if state == Qt.GestureState.GestureStarted:
            self._pinch_scale = self._shown_scale()
            self._pinch_rotation = self.rotation
            self._pinch_center = pinch.centerPoint()
            self._drag = None
        if state in (Qt.GestureState.GestureStarted, Qt.GestureState.GestureUpdated):
            base = self._base_scale()
            shown = clamp_zoom(self._pinch_scale * pinch.totalScaleFactor())
            self.zoom = shown / base if base > 0 else 1.0
            self.rotation = self._pinch_rotation + pinch.totalRotationAngle()
            center = pinch.centerPoint()
            if self._pinch_center is not None:
                self.pan_x += center.x() - self._pinch_center.x()
                self.pan_y += center.y() - self._pinch_center.y()
            self._pinch_center = center
            self.apply()
        event.accept()
        return True

    def eventFilter(self, watched, event):
        kind = event.type()
        if kind not in (QEvent.Type.KeyPress, QEvent.Type.KeyRelease):
            return False
        if event.key() != Qt.Key_Space or event.isAutoRepeat():
            return False
        if self._text_focus():
            return False
        window = self.window()
        if window is None or not window.isActiveWindow():
            return False
        self._space = kind == QEvent.Type.KeyPress
        self._cursor()
        return True

    def _emit_pick(self, pos):
        if self.on_pick is None:
            return
        hit = self.document_at(pos)
        if hit is None:
            self.on_pick(None, None)
            return
        self.on_pick(hit[0], hit[1])

    def _gesture(self, event):
        mods = event.modifiers()
        return gesture(
            _button_index(event.button()),
            space=self._space,
            shift=bool(mods & Qt.ShiftModifier),
            alt=bool(mods & Qt.AltModifier),
            ctrl=bool(mods & Qt.ControlModifier),
            meta=bool(mods & Qt.MetaModifier),
            pointer=_pointer_kind(event),
        )

    def _text_focus(self):
        widget = QApplication.focusWidget()
        return isinstance(widget, (QLineEdit, QSpinBox, QPlainTextEdit))

    def _cursor(self):
        if self._drag == "pan":
            self.setCursor(Qt.ClosedHandCursor)
        elif self._space:
            self.setCursor(Qt.OpenHandCursor)
        elif self._drag == "rotate":
            self.setCursor(Qt.SizeHorCursor)
        else:
            self.unsetCursor()


def _button_index(button):
    if button == Qt.MiddleButton:
        return 1
    if button == Qt.RightButton:
        return 2
    if button == Qt.LeftButton:
        return 0
    return -1


def _pointer_kind(event):
    try:
        kind = event.pointerType()
    except Exception:
        return "mouse"
    pen = QPointingDevice.PointerType.Pen
    eraser = getattr(QPointingDevice.PointerType, "Eraser", None)
    finger = getattr(QPointingDevice.PointerType, "Finger", None)
    if kind == pen or (eraser is not None and kind == eraser):
        return "pen"
    if finger is not None and kind == finger:
        return "touch"
    return "mouse"


class LabelDelegate(QStyledItemDelegate):
    def createEditor(self, parent, option, index):
        if index.column() != 1:
            return None
        return super().createEditor(parent, option, index)


class ObjectDelegate(QStyledItemDelegate):
    """The same guides as the layer tree. Names stay quiet, the selection white."""

    def sizeHint(self, option, index):
        return QSize(option.rect.width(), 22)

    def paint(self, painter, option, index):
        tree = option.widget
        if tree is not None and not isinstance(tree, ObjectOutliner):
            tree = tree.parent()
        item = tree.itemFromIndex(index) if isinstance(tree, ObjectOutliner) else None
        if item is None:
            return
        painter.save()
        painter.setRenderHint(QPainter.Antialiasing, False)
        rect = option.rect
        selected = bool(option.state & QStyle.StateFlag.State_Selected)
        role = index.data(Qt.UserRole + 1) or ""
        if selected:
            color = INK
        elif role == "target":
            color = WARP_PINK
        elif role == "window":
            color = WARP_PURPLE
        else:
            color = QUIET
        painter.fillRect(rect, QColor("#111111" if selected else "#000000"))
        depth = _item_depth(item)
        folder = item.childCount() > 0
        chain = _item_chain(item)
        pen = QPen(QColor(RULE))
        pen.setWidth(1)
        painter.setPen(pen)
        painter.setBrush(Qt.NoBrush)
        x0 = rect.x() + 4
        mid = rect.center().y()
        for level in range(depth):
            if level >= len(chain) or not _item_has_next(chain[level]):
                continue
            guide = x0 + level * GUIDE + GUIDE // 2
            painter.drawLine(guide, rect.top(), guide, rect.bottom())
        branch, disclosure, name_x = object_row_marks(rect, depth, folder)
        painter.drawLine(branch, rect.top(), branch, mid)
        if _item_has_next(item):
            painter.drawLine(branch, mid, branch, rect.bottom())
        if disclosure.isValid():
            painter.drawLine(branch, mid, disclosure.left(), mid)
        else:
            painter.drawLine(branch, mid, max(branch, name_x - 8), mid)
        mark = QColor(INK if selected else QUIET)
        painter.setPen(mark)
        if disclosure.isValid():
            painter.setBrush(Qt.NoBrush)
            painter.drawRect(disclosure)
            painter.drawLine(
                disclosure.left() + 2, disclosure.center().y(),
                disclosure.right() - 2, disclosure.center().y(),
            )
            if not item.isExpanded():
                painter.drawLine(
                    disclosure.center().x(), disclosure.top() + 2,
                    disclosure.center().x(), disclosure.bottom() - 2,
                )
        painter.setPen(QColor(color))
        painter.setFont(option.font)
        text = index.data(Qt.DisplayRole) or ""
        available = max(0, rect.right() - name_x - 4)
        shown = painter.fontMetrics().elidedText(text, Qt.ElideRight, available)
        painter.drawText(name_x, rect.top(), available, rect.height(), Qt.AlignVCenter, shown)
        text_width = painter.fontMetrics().horizontalAdvance(shown)
        rule = name_rule(name_x, text_width, rect.right() - 4, rect.bottom(), rect.right())
        if rule is not None:
            painter.setPen(QPen(QColor(RULE)))
            painter.drawLine(rule[0], rule[1], rule[2], rule[3])
        painter.restore()


class ObjectOutliner(QTreeWidget):
    """The level, filtered from the layer tree. The top of a group is in front."""

    def __init__(self, host):
        super().__init__()
        self.host = host
        self.setHeaderHidden(True)
        self.setIndentation(0)
        self.setRootIsDecorated(False)
        self.setAnimated(False)
        self.setUniformRowHeights(True)
        self.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.setDragDropMode(QAbstractItemView.InternalMove)
        self.setDefaultDropAction(Qt.MoveAction)
        self.setDragEnabled(True)
        self.setAcceptDrops(True)
        self.setDropIndicatorShown(True)
        self.setExpandsOnDoubleClick(False)
        self.setEditTriggers(QAbstractItemView.DoubleClicked)
        self.setItemDelegate(ObjectDelegate(self))
        self.setContextMenuPolicy(Qt.CustomContextMenu)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setFrameShape(QFrame.NoFrame)
        self.viewport().setMouseTracking(True)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        width = self.viewport().width()
        if width > 0 and self.columnWidth(0) != width:
            self.setColumnWidth(0, width)

    def _disclosure_rect(self, index):
        item = self.itemFromIndex(index)
        if item is None or item.childCount() == 0:
            return QRect()
        _branch, disclosure, _name = object_row_marks(
            self.visualRect(index), _item_depth(item), True,
        )
        return disclosure

    def mousePressEvent(self, event):
        if event.button() == Qt.LeftButton:
            pos = event.position().toPoint()
            index = self.indexAt(pos)
            box = self._disclosure_rect(index) if index.isValid() else QRect()
            if box.isValid() and box.contains(pos):
                item = self.itemFromIndex(index)
                item.setExpanded(not item.isExpanded())
                opened = item.isExpanded()
                self.host.status.showMessage("%s %s." % ("Opened" if opened else "Closed", item.text(0)))
                event.accept()
                return
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event):
        index = self.indexAt(event.position().toPoint())
        box = self._disclosure_rect(index) if index.isValid() else QRect()
        if box.isValid() and box.contains(event.position().toPoint()):
            item = self.itemFromIndex(index)
            tip = "Close folder" if item.isExpanded() else "Open folder"
            QToolTip.showText(event.globalPosition().toPoint(), tip, self)
        super().mouseMoveEvent(event)

    def dropEvent(self, event):
        moving = {item.data(0, Qt.UserRole) for item in self.selectedItems()}
        target = self.itemAt(event.position().toPoint())
        onto = self.dropIndicatorPosition() == QAbstractItemView.DropIndicatorPosition.OnItem
        if onto and target is not None and self._would_cycle(moving, target):
            event.ignore()
            self.host.status.showMessage("An object stays out of its own contents.")
            return
        super().dropEvent(event)
        QTimer.singleShot(0, self.host._apply_object_tree)

    def _would_cycle(self, moving, target):
        item = target
        while item is not None:
            if item.data(0, Qt.UserRole) in moving:
                return True
            item = item.parent()
        return False

    def rows(self):
        def group(parent):
            count = self.topLevelItemCount() if parent is None else parent.childCount()
            found = []
            for index in range(count):
                item = self.topLevelItem(index) if parent is None else parent.child(index)
                found.append({
                    "id": item.data(0, Qt.UserRole),
                    "children": group(item),
                })
            return found

        return group(None)


class NameDialog(QDialog):
    """The object name is confirmed here. A folder name is the starting text."""

    def __init__(self, parent, layer_names, name=""):
        super().__init__(parent)
        self.setWindowTitle("New object")
        layout = QVBoxLayout(self)
        title = QLabel("NAME")
        title.setObjectName("title")
        layout.addWidget(title)
        if name:
            layout.addWidget(QLabel("The folder name starts here. Change it when that name is wrong."))
        else:
            layout.addWidget(QLabel("Type the name before it is applied. The layers do not decide it."))
        self.edit = QLineEdit()
        self.edit.setPlaceholderText("Object name")
        if name:
            self.edit.setText(name)
        layout.addWidget(self.edit)
        shown = ", ".join(layer_names[:12])
        extra = len(layer_names) - 12
        if extra > 0:
            shown = "%s, and %d more" % (shown, extra)
        layout.addWidget(QLabel("%d layers: %s" % (len(layer_names), shown)))
        buttons = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        ok = QPushButton("Create")
        ok.setObjectName("export")
        ok.clicked.connect(self._accept)
        buttons.addWidget(cancel)
        buttons.addWidget(ok)
        layout.addLayout(buttons)
        self.edit.setFocus()
        if name:
            self.edit.selectAll()
        self.setMinimumWidth(420)

    def _accept(self):
        if not self.edit.text().strip():
            return
        self.accept()

    def name(self):
        return self.edit.text().strip()


class CutFolderDialog(QDialog):
    """Name the folder a cut becomes. The same box can add it as an object."""

    def __init__(self, parent, name):
        super().__init__(parent)
        self.setWindowTitle("Cut to folder")
        layout = QVBoxLayout(self)
        title = QLabel("NAME")
        title.setObjectName("title")
        layout.addWidget(title)
        layout.addWidget(QLabel("Name this folder. Change it when Cut is the wrong name."))
        self.edit = QLineEdit()
        self.edit.setPlaceholderText("Folder name")
        self.edit.setText(name or "")
        layout.addWidget(self.edit)
        self.as_object = QCheckBox("Make this an object")
        self.as_object.setObjectName("cutObject")
        self.as_object.setStyleSheet(
            "QCheckBox { color: #ffffff; spacing: 8px; }"
            "QCheckBox::indicator { width: 14px; height: 14px;"
            " border: 1px solid #ffffff; background: #000000; }"
            "QCheckBox::indicator:checked { background: #ffffff; }"
        )
        layout.addWidget(self.as_object)
        layout.addWidget(QLabel(
            "The object uses this name and shows up in the object list. Pictures are written on Export."
        ))
        buttons = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        ok = QPushButton("Cut")
        ok.setObjectName("export")
        ok.setDefault(True)
        ok.clicked.connect(self._accept)
        buttons.addWidget(cancel)
        buttons.addWidget(ok)
        layout.addLayout(buttons)
        self.edit.setFocus()
        if name:
            self.edit.selectAll()
        self.setMinimumWidth(420)

    def _accept(self):
        if not self.edit.text().strip():
            return
        self.accept()

    def name(self):
        return self.edit.text().strip()

    def wants_object(self):
        return self.as_object.isChecked()


class SlotNameDialog(QDialog):
    def __init__(self, parent):
        super().__init__(parent)
        self.setWindowTitle("Name a position")
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Name this position. The name is written in the object file."))
        self.edit = QLineEdit()
        self.edit.setPlaceholderText("Position name")
        layout.addWidget(self.edit)
        row = QHBoxLayout()
        row.addWidget(QLabel("State"))
        self.state = QSpinBox()
        self.state.setRange(0, 0xFFF)
        row.addWidget(self.state)
        row.addWidget(QLabel("Frame"))
        self.frame = QSpinBox()
        self.frame.setRange(0, 0xFFF)
        row.addWidget(self.frame)
        layout.addLayout(row)
        self.preview = QLabel("000000")
        layout.addWidget(self.preview)
        self.state.valueChanged.connect(self._preview)
        self.frame.valueChanged.connect(self._preview)
        buttons = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        ok = QPushButton("Name")
        ok.setObjectName("export")
        ok.clicked.connect(self._accept)
        buttons.addWidget(cancel)
        buttons.addWidget(ok)
        layout.addLayout(buttons)
        self.edit.setFocus()
        self.setMinimumWidth(360)

    def _preview(self, _value=0):
        self.preview.setText(slot_key(self.state.value(), self.frame.value()))

    def _accept(self):
        if not self.edit.text().strip():
            return
        self.accept()

    def values(self):
        return self.edit.text().strip(), int(self.state.value()), int(self.frame.value())


class AssignDialog(QDialog):
    def __init__(self, parent, count, state, frame):
        super().__init__(parent)
        self.setWindowTitle("Assign frame")
        self.state = state & 0xFFF
        self.frame = frame & 0xFFF
        self.mode = "stack"
        self.count = count
        layout = QVBoxLayout(self)
        title = QLabel("ASSIGN FRAME")
        title.setObjectName("title")
        layout.addWidget(title)
        layout.addWidget(QLabel(
            "%d layer%s. 000 rest, 001 hover, and 002 pressed are the button positions. 003 is after."
            % (count, "" if count == 1 else "s")
        ))
        states = QHBoxLayout()
        self.state_buttons = []
        for value, label in ((0, "rest"), (1, "hover"), (2, "pressed"), (3, "after")):
            button = QPushButton("%s %03X" % (label, value))
            button.clicked.connect(lambda _checked=False, number=value: self._set_state(number))
            states.addWidget(button)
            self.state_buttons.append((value, button))
        layout.addLayout(states)
        hex_row = QHBoxLayout()
        hex_row.addWidget(QLabel("State hex"))
        self.hex_edit = QLineEdit("%03X" % self.state)
        self.hex_edit.setMaxLength(3)
        self.hex_edit.textChanged.connect(self._hex_changed)
        hex_row.addWidget(self.hex_edit)
        layout.addLayout(hex_row)
        frame_row = QHBoxLayout()
        frame_row.addWidget(QLabel("Frame"))
        self.frame_spin = QSpinBox()
        self.frame_spin.setRange(0, 0xFFF)
        self.frame_spin.setValue(self.frame)
        self.frame_spin.valueChanged.connect(self._frame_changed)
        frame_row.addWidget(self.frame_spin)
        self.frame_hex = QLabel("%03X" % self.frame)
        frame_row.addWidget(self.frame_hex)
        layout.addLayout(frame_row)
        if count > 1:
            modes = QHBoxLayout()
            self.stack_button = QPushButton("Same picture")
            self.sequence_button = QPushButton("Next frames")
            self.stack_button.clicked.connect(lambda: self._set_mode("stack"))
            self.sequence_button.clicked.connect(lambda: self._set_mode("sequence"))
            modes.addWidget(self.stack_button)
            modes.addWidget(self.sequence_button)
            layout.addLayout(modes)
        self.preview = QLabel("")
        layout.addWidget(self.preview)
        buttons = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        ok = QPushButton("Assign")
        ok.setObjectName("export")
        ok.clicked.connect(self.accept)
        buttons.addWidget(cancel)
        buttons.addWidget(ok)
        layout.addLayout(buttons)
        self._refresh()
        self.setMinimumWidth(420)

    def _set_state(self, number):
        self.state = number & 0xFFF
        self.hex_edit.blockSignals(True)
        self.hex_edit.setText("%03X" % self.state)
        self.hex_edit.blockSignals(False)
        self._refresh()

    def _hex_changed(self, text):
        cleaned = "".join(char for char in text if char.lower() in "0123456789abcdef")
        if cleaned != text:
            self.hex_edit.setText(cleaned)
            return
        if cleaned:
            self.state = int(cleaned, 16) & 0xFFF
            self._refresh()

    def _frame_changed(self, value):
        self.frame = int(value) & 0xFFF
        self._refresh()

    def _set_mode(self, mode):
        self.mode = mode
        self._refresh()

    def _refresh(self):
        for value, button in self.state_buttons:
            button.setProperty("current", value == self.state)
            button.style().unpolish(button)
            button.style().polish(button)
        if self.count > 1:
            self.stack_button.setProperty("current", self.mode == "stack")
            self.sequence_button.setProperty("current", self.mode == "sequence")
            for button in (self.stack_button, self.sequence_button):
                button.style().unpolish(button)
                button.style().polish(button)
        self.frame_hex.setText("%03X" % self.frame)
        if self.mode == "sequence" and self.count > 1:
            end = slot_key(self.state, (self.frame + self.count - 1) & 0xFFF)
            self.preview.setText("%s … %s" % (slot_key(self.state, self.frame), end))
        else:
            self.preview.setText(slot_key(self.state, self.frame))

    @staticmethod
    def ask(parent, count, state, frame):
        dialog = AssignDialog(parent, count, state, frame)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return None
        return dialog.state, dialog.frame, dialog.mode


class _PictureJob(QThread):
    """Build the fit picture off the interface thread.

    Hide, show, and frame changes were rebuilding it on the interface thread.
    A big drawing froze the window, and the frozen process was killed.
    """

    ready = Signal(int, object, str)

    def __init__(self, generation, width, height, layers, focus, edge, flags, showing):
        super().__init__()
        self.generation = int(generation)
        self.width = width
        self.height = height
        self.layers = layers
        self.focus = focus
        self.edge = edge
        # Copied on the interface thread. The build must not read a later click.
        self.flags = flags
        self.showing = showing

    def run(self):
        image = None
        error = ""
        try:
            if self.showing:
                image = composite_scene(
                    self.width, self.height, self.layers, self.focus,
                    max_edge=self.edge, flags=self.flags,
                )
        except Exception:
            error = traceback.format_exc()
        self.ready.emit(self.generation, image, error)


class MainWindow(QMainWindow):
    def __init__(self, dev=False):
        super().__init__()
        self._split_custom = False
        self._split_guard = False
        self._split_saved = None
        self._split_ready = False
        self._picture_job = None
        self._picture_generation = 0
        self._picture_pending = False
        self._save_timer = QTimer(self)
        self._save_timer.setSingleShot(True)
        self._save_timer.setInterval(200)
        self._save_timer.timeout.connect(self._save_session)
        self.dev = dev
        self.art = None
        self.objects = []
        self.picked = set()
        self.ink = 0
        self._flat = None
        self._loader = None
        self._loading = False
        self._saver = None
        self.history = History()
        self._fingers = FingerChords()
        self._watch_base = None
        self._watch_pending = None
        self.setWindowTitle("VMI STUDIO")
        self.resize(1280, 800)
        self.model = LayerModel()
        self.model.picked = self.picked
        self.tree = Outliner(self.model, self)
        self.picture = PictureView()
        self.picture.on_resize = self._picture_resized
        self.picture.on_pick = self.choose_picture
        self._preview_edge = None
        self.caption = QLabel("")
        self.caption.setObjectName("title")
        self.timeline_title = QLabel("TIMELINE", self)
        self.timeline_title.setObjectName("title")
        self.timeline_title.hide()
        self.timeline = QListWidget(self)
        self.timeline.hide()
        self.timeline.currentRowChanged.connect(self._timeline_chosen)
        self._timeline_lock = False
        self.earlier_button = QPushButton("Earlier", self)
        self.later_button = QPushButton("Later", self)
        self.earlier_button.hide()
        self.later_button.hide()
        self.earlier_button.clicked.connect(lambda: self._move_frame(-1))
        self.later_button.clicked.connect(lambda: self._move_frame(1))
        self.objects_list = ObjectOutliner(self)
        delete_key = QAction(self.objects_list)
        delete_key.setShortcut(QKeySequence.Delete)
        delete_key.setShortcutContext(Qt.WidgetShortcut)
        delete_key.triggered.connect(self.delete_objects)
        self.objects_list.addAction(delete_key)
        self.objects_list.itemChanged.connect(self._object_renamed)
        self.objects_list.currentItemChanged.connect(lambda *_: self._show_slots())
        self.objects_list.setContextMenuPolicy(Qt.CustomContextMenu)
        self.objects_list.customContextMenuRequested.connect(self._object_menu)
        self.slots = QTreeWidget()
        self.slots.setColumnCount(3)
        self.slots.setHeaderLabels(["Position", "Label", "Layers"])
        self.slots.setRootIsDecorated(False)
        self.slots.setEditTriggers(QAbstractItemView.DoubleClicked)
        self.slots.setItemDelegate(LabelDelegate(self.slots))
        self.slots.itemChanged.connect(self._slot_renamed)
        self.slot_note = QLabel(
            "000 rest, 001 hover, and 002 pressed are the button positions, like the first frames of an Animate button. 003 is after. Double-click a label to rename it."
        )
        self.slot_note.setWordWrap(True)
        self._build_level_fields()
        self.arrange_locked = True
        self.arrange_lock = ArrangeLock()
        self.arrange_lock.toggled.connect(self._arrange_toggled)
        self.split = QSplitter(Qt.Horizontal)
        self.split.addWidget(self._pane("LAYERS", self.tree, self.arrange_lock))
        picture_body = QWidget()
        picture_layout = QVBoxLayout(picture_body)
        picture_layout.setContentsMargins(0, 0, 0, 0)
        picture_layout.setSpacing(4)
        picture_layout.addWidget(self.picture, 1)
        self._cut_target = None
        self.cut_bar = self._cut_bar()
        picture_layout.addWidget(self.cut_bar)
        picture_layout.addWidget(self.caption)
        app = QApplication.instance()
        if app is not None:
            app.installEventFilter(self)
        self.split.addWidget(self._pane("PICTURE", picture_body))
        self.split.addWidget(self._pane("OBJECTS", self._objects_body()))
        self.split.setHandleWidth(GUTTER)
        self.split.setChildrenCollapsible(False)
        self.split.splitterMoved.connect(self._split_dragged)
        for index in range(3):
            self.split.widget(index).setMinimumWidth(PANE_MIN)
        screen = QApplication.primaryScreen()
        if screen_pixels(screen) >= WIDE_SCREEN:
            for index, factor in enumerate((9, 82, 9)):
                self.split.setStretchFactor(index, factor)
        self.setCentralWidget(self.split)
        self.status = self.statusBar()
        self.status.showMessage("Open a .clip, .psd, .kra, .xcf, or .vmib file.")
        self._font_id = fonts.current_id()
        self._font_actions = {}
        self._font_previewed = False
        self._menu_bar()
        selection = self.tree.selectionModel()
        if selection is not None:
            selection.currentChanged.connect(self._layer_current)
        if dev:
            self._watch_base = self._snapshot()
            timer = QTimer(self)
            timer.setInterval(400)
            timer.timeout.connect(self._watch)
            timer.start()
            self._timer = timer

    def _pane(self, title, body, corner=None):
        wrap = QWidget()
        layout = QVBoxLayout(wrap)
        layout.setContentsMargins(8, 8, 8, 8)
        layout.setSpacing(6)
        label = QLabel(title)
        label.setObjectName("title")
        if corner is None:
            layout.addWidget(label)
        else:
            row = QHBoxLayout()
            row.setContentsMargins(0, 0, 0, 0)
            row.setSpacing(6)
            row.addWidget(label, 1)
            row.addWidget(corner, 0, Qt.AlignRight | Qt.AlignVCenter)
            layout.addLayout(row)
        layout.addWidget(body, 1)
        # The long notes and tables must not force a pane wider than the gutter allows.
        wrap.setMinimumWidth(PANE_MIN)
        wrap.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Expanding)
        return wrap

    def _set_arrange_locked(self, locked):
        self.arrange_locked = bool(locked)
        self.arrange_lock.blockSignals(True)
        self.arrange_lock.setChecked(not self.arrange_locked)
        self.arrange_lock.blockSignals(False)
        self.arrange_lock.update()

    def _arrange_toggled(self, unlocked):
        self.arrange_locked = not bool(unlocked)
        self.arrange_lock.update()
        if self.arrange_locked:
            self.status.showMessage("Arrangement locked.")
        else:
            self.status.showMessage("Arrangement unlocked. Drag a layer or folder to move it.")
        if self.art:
            self._save_session()

    def _field_label(self, text):
        label = QLabel(text)
        label.setObjectName("title")
        return label

    def _choice(self, pairs):
        box = QComboBox()
        box.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Fixed)
        box.setMinimumWidth(0)
        box.setMinimumContentsLength(8)
        box.setSizeAdjustPolicy(QComboBox.SizeAdjustPolicy.AdjustToMinimumContentsLengthWithIcon)
        for label, value in pairs:
            box.addItem(label, value)
        return box

    def _build_level_fields(self):
        """One form for the selected object, and one playlist for the level."""
        self.playlist = ""
        self._field_guard = False
        self.playlist_box = self._choice((
            ("none", ""),
            ("menu", "menu"),
            ("battle", "battle"),
        ))
        self.playlist_box.setToolTip("Webamp for this level. menu and battle are the two Beetle Game lists.")
        self.object_name = QLineEdit()
        self.object_name.setPlaceholderText("Object name")
        self.object_name.setMinimumWidth(0)
        self.object_kind = self._choice((
            ("from the layers", ""),
            ("static", "static"),
            ("button", "button"),
            ("sequence", "sequence"),
            ("warp target", "warp target"),
        ))
        self.kind_note = QLabel("Pick an object.")
        self.kind_note.setObjectName("title")
        self.kind_note.setWordWrap(True)
        self.object_warp = self._choice((
            ("none", ""),
            ("mask", "mask"),
            ("warp map", "warp map"),
        ))
        self.object_warp.setToolTip("A mask is the painted alpha. A warp map is the quad.")
        self.object_sound = self._choice(
            (("none", ""),) + tuple((label, ident) for ident, label in SOUND_EFFECTS)
        )
        self.object_sound.setToolTip("Battle sound effects from Beetle Game.")
        self.playlist_box.currentIndexChanged.connect(self._playlist_chosen)
        self.object_name.editingFinished.connect(self._name_edited)
        self.object_kind.currentIndexChanged.connect(self._kind_chosen)
        self.object_warp.currentIndexChanged.connect(self._warp_chosen)
        self.object_sound.currentIndexChanged.connect(self._sound_chosen)
        self.objects_list.setMinimumHeight(160)
        self.objects_list.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Expanding)
        self.slots.setMaximumHeight(96)

    def _objects_body(self):
        """The level outliner fills the top of this pane. Fields sit under it."""
        body = QWidget()
        layout = QVBoxLayout(body)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        self.object_note = QLabel("The top of a group is in front. Drop an object onto another to put it inside.")
        self.object_note.setWordWrap(True)
        layout.addWidget(self.object_note)
        layout.addWidget(self.objects_list, 1)

        detail = QWidget()
        form = QVBoxLayout(detail)
        form.setContentsMargins(0, 0, 0, 0)
        form.setSpacing(6)
        form.addWidget(self._field_label("NAME"))
        form.addWidget(self.object_name)
        form.addWidget(self._field_label("TYPE"))
        form.addWidget(self.object_kind)
        form.addWidget(self.kind_note)
        form.addWidget(self._field_label("WARP LAYER"))
        form.addWidget(self.object_warp)
        form.addWidget(self._field_label("SOUND"))
        form.addWidget(self.object_sound)
        form.addWidget(self._field_label("PLAYLIST"))
        form.addWidget(self.playlist_box)
        form.addWidget(self.slot_note)
        form.addWidget(self.slots)
        add_slot = QPushButton("Add slot")
        add_slot.clicked.connect(self.add_slot)
        form.addWidget(add_slot)
        export = QPushButton("Export")
        export.setObjectName("export")
        export.clicked.connect(self.export_scene)
        form.addWidget(export)
        detail.setMinimumWidth(0)
        detail.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.NoFrame)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        scroll.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        scroll.setMaximumHeight(220)
        scroll.setWidget(detail)
        layout.addWidget(scroll, 0)
        body.setMinimumWidth(0)
        body.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Expanding)
        return body

    def _menu_bar(self):
        file_menu = self.menuBar().addMenu("File")
        open_action = QAction("Open", self)
        open_action.setShortcut(QKeySequence.Open)
        open_action.triggered.connect(self.choose_file)
        save_action = QAction("Save blueprint\tCtrl+S", self)
        save_action.triggered.connect(self.save_blueprint)
        save_as_action = QAction("Save blueprint as", self)
        save_as_action.triggered.connect(self.save_blueprint_as)
        export_action = QAction("Export scene", self)
        export_action.setShortcut(QKeySequence("Ctrl+E"))
        export_action.triggered.connect(self.export_scene)
        quit_action = QAction("Quit", self)
        quit_action.setShortcut(QKeySequence.Quit)
        quit_action.triggered.connect(self.close)
        file_menu.addAction(open_action)
        file_menu.addAction(save_action)
        file_menu.addAction(save_as_action)
        file_menu.addAction(export_action)
        file_menu.addSeparator()
        file_menu.addAction(quit_action)
        edit_menu = self.menuBar().addMenu("Edit")
        undo_action = QAction("Undo\tCtrl+Z", self)
        undo_action.triggered.connect(self.undo)
        redo_action = QAction("Redo\tCtrl+Shift+Z", self)
        redo_action.triggered.connect(self.redo)
        edit_menu.addAction(undo_action)
        edit_menu.addAction(redo_action)
        view_menu = self.menuBar().addMenu("View")
        zoom_in = QAction("Zoom in", self)
        zoom_in.setShortcut(QKeySequence.ZoomIn)
        zoom_in.triggered.connect(lambda: self._zoom_picture(ZOOM_STEP))
        zoom_out = QAction("Zoom out", self)
        zoom_out.setShortcut(QKeySequence.ZoomOut)
        zoom_out.triggered.connect(lambda: self._zoom_picture(1.0 / ZOOM_STEP))
        fit_action = QAction("Fit picture", self)
        fit_action.setShortcut(QKeySequence("Ctrl+0"))
        fit_action.triggered.connect(lambda: self.picture.set_fit(True))
        actual_action = QAction("Actual size", self)
        actual_action.setShortcut(QKeySequence("Ctrl+1"))
        actual_action.triggered.connect(lambda: self.picture.set_fit(False))
        view_menu.addAction(zoom_in)
        view_menu.addAction(zoom_out)
        view_menu.addAction(fit_action)
        view_menu.addAction(actual_action)
        view_menu.addSeparator()
        pan_hint = QAction("Pan    Space + drag, or middle-click", self)
        pan_hint.setEnabled(False)
        rotate_hint = QAction("Rotate    Shift + drag", self)
        rotate_hint.setEnabled(False)
        view_menu.addAction(pan_hint)
        view_menu.addAction(rotate_hint)
        self._font_menu()
        layer_menu = self.menuBar().addMenu("Layer")
        group_action = QAction("Create folder and insert layer", self)
        group_action.setShortcut(QKeySequence("Ctrl+G"))
        group_action.setShortcutContext(Qt.WindowShortcut)
        group_action.triggered.connect(self.group_selection)
        anim_action = QAction("Animation folder", self)
        anim_action.triggered.connect(self.mark_current_animation)
        layer_menu.addAction(group_action)
        layer_menu.addAction(anim_action)

    def _font_menu(self):
        menu = self.menuBar().addMenu("Font")
        self._font_group = QActionGroup(self)
        self._font_group.setExclusive(True)
        buckets = {}
        system = None
        for face in fonts.catalog():
            if face.id == fonts.DEFAULT_ID:
                system = face
                continue
            buckets.setdefault(face.category, []).append(face)
        if system is not None:
            self._add_font_action(menu, system)
            menu.addSeparator()
        for category in fonts.CATEGORY_ORDER:
            rows = buckets.pop(category, [])
            if rows:
                self._fill_font_menu(menu.addMenu(fonts.CATEGORY_LABELS.get(category, category)), rows)
        for category, rows in buckets.items():
            self._fill_font_menu(menu.addMenu(category), rows)
        menu.aboutToShow.connect(self._preview_font_menu)

    def _fill_font_menu(self, menu, rows):
        for face in sorted(rows, key=lambda item: item.name.lower()):
            self._add_font_action(menu, face)
        menu.aboutToShow.connect(self._preview_font_menu)

    def _add_font_action(self, menu, face):
        action = QAction(face.name, self)
        action.setCheckable(True)
        action.setActionGroup(self._font_group)
        action.triggered.connect(lambda _checked=False, ident=face.id: self.set_ui_font(ident))
        action.setChecked(face.id == self._font_id)
        menu.addAction(action)
        self._font_actions[face.id] = action

    def _preview_font_menu(self):
        if self._font_previewed:
            return
        self._font_previewed = True
        for ident, action in self._font_actions.items():
            family = fonts.preview_family(ident)
            if not family:
                continue
            font = QFont(family)
            font.setPixelSize(12)
            action.setFont(font)

    def set_ui_font(self, ident):
        if ident == self._font_id:
            return
        face = fonts.get(ident)
        if face is None:
            return
        self.status.showMessage("Loading %s" % face.name)
        QApplication.processEvents()
        try:
            loaded, family = fonts.activate(ident)
        except Exception:
            self.status.showMessage("Could not load %s." % face.name)
            self._check_font()
            return
        _paint_font(QApplication.instance(), loaded, family)
        self._font_id = loaded.id
        self._check_font()
        action = self._font_actions.get(loaded.id)
        if action is not None:
            preview = QFont(family)
            preview.setPixelSize(12)
            action.setFont(preview)
        self._persist_font()
        self.status.showMessage("UI font: %s" % loaded.name)

    def _check_font(self):
        action = self._font_actions.get(self._font_id)
        if action is not None:
            action.setChecked(True)

    def _persist_font(self):
        if self.art:
            self._save_session()
            return
        data = load() or {}
        if not isinstance(data, dict):
            data = {}
        data["font"] = self._font_id
        save(data)

    def choose_file(self):
        start = ""
        if self.art and self.art.path:
            start = os.path.dirname(self.art.path)
        path, _selected = QFileDialog.getOpenFileName(
            self,
            "Open drawing",
            start,
            "Projects (*.vmib *.clip *.psd *.psb *.kra *.xcf);;All files (*)",
        )
        if path:
            self.open_path(path)

    def save_blueprint(self):
        """Ctrl+S. An open blueprint is overwritten. A drawing is saved beside it."""
        if not self.art:
            self.status.showMessage("Open a drawing first.")
            return
        if self.art.kind == "vmib" and self.art.path:
            self._write_blueprint(self.art.path)
            return
        self.save_blueprint_as()

    def save_blueprint_as(self):
        if not self.art:
            self.status.showMessage("Open a drawing first.")
            return
        start = os.path.dirname(self.art.path) if self.art.path else os.path.expanduser("~")
        stem = os.path.splitext(self.art.file_name or "project")[0]
        path, _selected = QFileDialog.getSaveFileName(
            self,
            "Save VMI blueprint",
            os.path.join(start, stem + ".vmib"),
            "VMI blueprint (*.vmib)",
        )
        if not path:
            return
        if os.path.splitext(path)[1].lower() != ".vmib":
            path += ".vmib"
        if self.art.path and self.art.kind != "vmib" and os.path.exists(self.art.path):
            try:
                if os.path.samefile(path, self.art.path):
                    self.status.showMessage("A blueprint is a new file. The drawing stays as it is.")
                    return
            except OSError:
                pass
        self._write_blueprint(path)

    def _write_blueprint(self, path):
        if self._saver is not None and self._saver.isRunning():
            self.status.showMessage("Still saving the blueprint.")
            return
        try:
            document, blobs = prepare_blueprint(
                self.art, self.objects, self.playlist, self.arrange_locked
            )
        except Exception as exc:
            self.status.showMessage(str(exc))
            return
        self._saver = SaveThread(path, document, blobs)
        self._saver.ok.connect(self._blueprint_saved)
        self._saver.bad.connect(self._blueprint_failed)
        self._saver.start()
        self.status.showMessage("Saving %s." % os.path.basename(path))

    def _blueprint_saved(self, path):
        if not self.art:
            return
        source = {}
        if self._saver is not None:
            source = self._saver.document.get("source") or {}
        self.art.path = os.path.abspath(path)
        self.art.file_name = os.path.basename(path)
        self.art.kind = "vmib"
        self.art.project = {
            "objects": list(self.objects),
            "playlist": self.playlist,
            "arrange_locked": self.arrange_locked,
            "source_file": source.get("file") or "",
            "source_kind": source.get("kind") or "",
        }
        self.setWindowTitle("VMI STUDIO — %s" % self.art.file_name)
        self._save_session()
        self.status.showMessage("Saved %s." % self.art.file_name)

    def _blueprint_failed(self, message):
        self.status.showMessage(message)

    def open_path(self, path):
        if self._loading:
            self.status.showMessage("Still reading.")
            return
        self._loading = True
        QApplication.setOverrideCursor(Qt.WaitCursor)
        self.status.showMessage("Reading %s" % os.path.basename(path))
        self._loader = LoadThread(path)
        self._loader.ok.connect(self._opened)
        self._loader.bad.connect(self._open_failed)
        self._loader.note.connect(self.status.showMessage)
        self._loader.start()

    def _opened(self, art):
        self._loading = False
        QApplication.restoreOverrideCursor()
        self.show_file(art)

    def _open_failed(self, message):
        self._loading = False
        QApplication.restoreOverrideCursor()
        self.status.showMessage(message)
        QMessageBox.warning(self, "VMI STUDIO", message)

    def _screen_width(self):
        return screen_pixels(self.screen() or QApplication.primaryScreen())

    def _clear_split_guard(self):
        self._split_guard = False

    def _split_dragged(self, _pos, _index):
        if self._split_guard:
            return
        self._split_custom = True
        for index, size in enumerate(self.split.sizes()):
            self.split.setStretchFactor(index, max(1, int(size)))

    def _apply_split(self, saved=None):
        if self._split_custom:
            return
        width = self.split.width()
        sizes = choose_split(width, saved, self._screen_width())
        if not sizes:
            return
        fitting = saved_split_fits(saved, width)
        target = [int(value) for value in sizes]
        if [int(value) for value in self.split.sizes()] != target:
            # splitterMoved can arrive after setSizes returns. Keep the guard
            # until the next turn so that move is not saved as a user drag.
            self._split_guard = True
            self.split.setSizes(target)
            QTimer.singleShot(0, self._clear_split_guard)
        if fitting:
            self._split_custom = True
            for index, size in enumerate(target):
                self.split.setStretchFactor(index, max(1, int(size)))
        else:
            for index, factor in enumerate((9, 82, 9)):
                self.split.setStretchFactor(index, factor)

    def _open_wide(self):
        offscreen = os.environ.get("QT_QPA_PLATFORM") == "offscreen"
        if not offscreen:
            self._stay_wide()
            # The window manager restores the last frame after map. Ask again
            # once that restore has landed, or the 4K gutters get the old size.
            QTimer.singleShot(400, self._stay_wide)
            QTimer.singleShot(1500, self._stay_wide)
        QTimer.singleShot(0, lambda: self._apply_split(self._split_saved))

    def _stay_wide(self):
        if os.environ.get("QT_QPA_PLATFORM") == "offscreen":
            return
        screen = QApplication.primaryScreen()
        if screen_pixels(screen) < WIDE_SCREEN:
            return
        if self.screen() is not screen:
            self.setScreen(screen)
        if not self.isMaximized():
            self.showMaximized()
        self._apply_split(self._split_saved)

    def showEvent(self, event):
        super().showEvent(event)
        if self._split_ready:
            return
        self._split_ready = True
        QTimer.singleShot(0, self._open_wide)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        if getattr(self, "split", None) is None or self._split_guard or self._split_custom:
            return
        self._apply_split(self._split_saved)

    def show_file(self, art, restore=True):
        self.history.clear()
        reset_plate_cache()
        self.art = art
        self.picked.clear()
        self.picture.set_document(art.width, art.height)
        self.picture.reset_view()
        session = load() if restore else None
        same = bool(
            session
            and session.get("path") == art.path
            and session.get("signature") == signature(art.layers)
        )
        if same and session.get("visible"):
            apply_visibility(art.layers, session["visible"])
        if same and session.get("animations"):
            apply_animations(art.layers, session["animations"])
        if same:
            apply_blends(art.layers, session.get("blends"))
            apply_track_flags(art.layers, session.get("mute"), session.get("solo"))
            if "clips" in session:
                apply_clips(art.layers, session.get("clips"))
        if session and session.get("tree_width"):
            self.tree.tree_width = max(MIN_TREE, int(session["tree_width"]))
        self.model.set_layers(art.layers)
        project = getattr(art, "project", None)
        if same:
            for ident in session.get("picked") or []:
                if find_node(art.layers, ident):
                    self.picked.add(ident)
            self.objects = [
                obj for obj in self._objects_from(session.get("objects") or []) if obj.explicit
            ]
            self._bump_ids()
            self.playlist = clean_playlist(session.get("playlist"))
            if "arrange_locked" in session:
                self._set_arrange_locked(session["arrange_locked"])
            else:
                self._set_arrange_locked(True)
        elif project:
            self.objects = list(project.get("objects") or [])
            self._bump_ids()
            self.playlist = clean_playlist(project.get("playlist"))
            self._set_arrange_locked(bool(project.get("arrange_locked", True)))
        else:
            reset_object_ids(1)
            self.objects = []
            self.playlist = ""
            self._set_arrange_locked(True)
        self._show_playlist()
        version_ok = bool(same and session and session.get("split_version") == SPLIT_VERSION)
        saved_split = session.get("splitter") if version_ok else None
        if not self._split_custom:
            if saved_split_fits(saved_split, self.split.width()):
                self._split_saved = saved_split
            else:
                self._split_saved = None
            self._apply_split(self._split_saved)
        self._fill_objects()
        self.recomposite()
        self.setWindowTitle("VMI STUDIO — %s" % art.file_name)
        opened = "%s  %d×%d  %s" % (art.file_name, art.width, art.height, art.note or "Opened.")
        targets, windows = warp_counts(art.layers)
        if targets or windows:
            opened += "  Warp targets: %d. Windows: %d." % (targets, windows)
        self.status.showMessage(opened)
        if same and session.get("selected"):
            self._select_layer(session["selected"])
        self._save_session()

    def _objects_from(self, rows):
        known = {node.id for node in walk(self.art.layers)}
        objects = []
        for row in rows:
            if not isinstance(row, dict):
                continue
            ids = [ident for ident in row.get("layer_ids") or [] if ident in known]
            assign = {}
            for ident, pos in (row.get("assign") or {}).items():
                if ident in known and isinstance(pos, (list, tuple)) and len(pos) >= 2:
                    assign[ident] = (int(pos[0]) & 0xFFF, int(pos[1]) & 0xFFF)
            labels = {}
            for key, value in (row.get("labels") or {}).items():
                if isinstance(key, str) and isinstance(value, str) and value.strip():
                    labels[key] = value.strip()
            objects.append(DeskObject(
                row.get("id") or "o0",
                row.get("name") or "object",
                ids,
                assign,
                row.get("origin") or "",
                labels,
                explicit=bool(row.get("explicit")),
                parent=row.get("parent") or "",
                stack=int(row.get("stack") or 0),
                role=row.get("role") or "",
                kind=row.get("kind") or "",
                warp_layer=row.get("warp_layer") or "",
                sound=row.get("sound") or "",
            ))
        return objects

    def _bump_ids(self):
        highest = 0
        for obj in self.objects:
            if isinstance(obj.id, str) and obj.id.startswith("o") and obj.id[1:].isdigit():
                highest = max(highest, int(obj.id[1:]))
        reset_object_ids(highest + 1)

    def _picture_edge(self):
        """Fit view composites to the screen. Actual size keeps the document."""
        if not self.picture.fit:
            return None
        size = self.picture.viewport().size()
        edge = max(size.width(), size.height())
        if edge < 64:
            return 1600
        ratio = self.devicePixelRatioF() or 1.0
        # The fit view never needs a plate bigger than the 4K screen.
        return min(3840, max(64, int(edge * ratio)))

    def _picture_resized(self):
        if not self.art or getattr(self, "_loading", False):
            return
        edge = self._picture_edge()
        old = self._preview_edge
        if edge == old:
            return
        if edge and old and abs(edge - old) < max(32, old * 0.12):
            return
        self.recomposite()

    def _picture_sync(self):
        """Tests build the picture before they look at it. The live window does not."""
        return os.environ.get("QT_QPA_PLATFORM") == "offscreen"

    def _job_alive(self):
        """True while the picture thread still exists. A deleted thread is not alive."""
        job = self._picture_job
        if job is None:
            return False
        try:
            from shiboken6 import isValid
            if not isValid(job):
                self._picture_job = None
                return False
        except Exception:
            self._picture_job = None
            return False
        try:
            running = job.isRunning()
        except RuntimeError:
            self._picture_job = None
            return False
        return bool(running)

    def recomposite(self, notice=None):
        if not self.art:
            self._picture_generation += 1
            self.ink = 0
            self._flat = None
            self.picture.clear_message("Open a drawing.")
            self._caption()
            return
        self._picture_generation += 1
        generation = self._picture_generation
        if self._picture_sync():
            self._finish_picture(generation, self._build_picture(), "")
            return
        self._picture_pending = True
        self.picture.set_busy(True, notice or PICTURE_WAIT)
        if self._job_alive():
            return
        self._start_picture_job()

    def _picture_request(self):
        """Focus, fit edge, and the hide/mute/solo copy for one picture build."""
        focus = self._focus_id()
        edge = self._picture_edge()
        self._preview_edge = edge
        flags = capture_flags(self.art.layers)
        showing = bool(preview_paint(self.art.layers, focus))
        return focus, edge, flags, showing

    def _build_picture(self):
        focus, edge, flags, showing = self._picture_request()
        if not showing:
            return None
        return composite_scene(
            self.art.width, self.art.height, self.art.layers, focus,
            max_edge=edge, flags=flags,
        )

    def _start_picture_job(self):
        if not self.art:
            return
        self._picture_pending = False
        generation = self._picture_generation
        focus, edge, flags, showing = self._picture_request()
        job = _PictureJob(
            generation, self.art.width, self.art.height, self.art.layers,
            focus, edge, flags, showing,
        )
        job.ready.connect(self._finish_picture)
        job.finished.connect(lambda job=job: self._drop_picture_job(job))
        self._picture_job = job
        self.picture.set_busy(True)
        job.start()

    def _drop_picture_job(self, job):
        """Forget the thread, then delete it. The next click must not call a dead one."""
        if self._picture_job is job:
            self._picture_job = None
        job.deleteLater()

    def _finish_picture(self, generation, image, error):
        current = generation == self._picture_generation and self.art
        if error and current:
            print(error, file=sys.stderr)
            self.status.showMessage("The picture could not be built.")
        elif current:
            showing = preview_paint(self.art.layers, self._focus_id())
            self._flat = image
            self.ink = ink_count(self._flat)
            if self._flat is None:
                if showing and not loaded_paint(self.art.layers):
                    self.picture.clear_message("This file has no layer pictures.")
                else:
                    self.picture.clear_message("Nothing visible.")
            else:
                shown = present(self._flat).convert("RGBA")
                self.picture.set_image(_pixmap(shown))
            self._caption()
            self.tree.viewport().update()
            if self.status.currentMessage() == REST_WAIT:
                self.status.showMessage("Solo off. Every track is back in the picture.")
        follow = self._picture_pending and self.art and not self._picture_sync()
        if follow:
            self._picture_pending = False
            QTimer.singleShot(0, self._start_picture_job)
        else:
            self.picture.set_busy(False)

    def _caption(self):
        if not self.art:
            self.caption.setText("")
            return
        index = self.tree.currentIndex()
        node = self.model.node_of(index) if index.isValid() else None
        if node and node.omit in ("hidden", "hidden folder"):
            self.caption.setText("%s is hidden." % node.name)
            return
        folder = owning_animation(self.art.layers, node.id) if node else None
        if folder is not None:
            cel = cel_of(folder, node.id) if node and node.id != folder.id else None
            visible = [item for item in timeline_cels(folder) if item.visible]
            if cel is None and visible:
                cel = visible[0]
            if cel is not None and cel in visible:
                state, _frame = parse_sf(folder.name)
                self.caption.setText("%s  %s  %s" % (folder.name, slot_key(state, visible.index(cel)), cel.name))
                return
        if self.ink > 0:
            self.caption.setText("Visible stack")
            return
        if preview_paint(self.art.layers):
            self.caption.setText("This file has no layer pictures.")
            return
        self.caption.setText("Nothing visible.")

    def _layer_current(self, *_args):
        if self._timeline_lock:
            return
        self._caption()
        self._refresh_timeline()
        if self.art and animation_folders(self.art.layers):
            self.recomposite()

    def _refresh_timeline(self):
        if self._timeline_lock:
            return
        self._timeline_lock = True
        self.timeline.clear()
        folder = None
        node = None
        if self.art:
            index = self.tree.currentIndex()
            node = self.model.node_of(index) if index.isValid() else None
            if node is not None:
                folder = owning_animation(self.art.layers, node.id)
        self.earlier_button.setEnabled(folder is not None)
        self.later_button.setEnabled(folder is not None)
        if folder is None:
            self.timeline_title.setText("TIMELINE")
            hint = QListWidgetItem("Mark a folder as an animation.")
            hint.setFlags(Qt.NoItemFlags)
            self.timeline.addItem(hint)
            self._timeline_lock = False
            return
        self.timeline_title.setText(folder.name)
        state, _frame = parse_sf(folder.name)
        number = 0
        current_row = 0
        for row, cel in enumerate(timeline_cels(folder)):
            if cel.visible:
                label = "%s  %s" % (slot_key(state, number), cel.name)
                number += 1
            else:
                label = "hidden  %s" % cel.name
            item = QListWidgetItem(label)
            item.setData(Qt.UserRole, cel.id)
            self.timeline.addItem(item)
            if node is not None and (node.id == cel.id or cel_of(folder, node.id) is cel):
                current_row = row
        if self.timeline.count():
            self.timeline.setCurrentRow(current_row)
        self._timeline_lock = False

    def _timeline_chosen(self, row):
        if self._timeline_lock or row < 0 or not self.art:
            return
        item = self.timeline.item(row)
        ident = item.data(Qt.UserRole) if item is not None else None
        if not ident:
            return
        self._select_layer(ident)

    def _move_frame(self, delta):
        if not self.art:
            return
        index = self.tree.currentIndex()
        node = self.model.node_of(index) if index.isValid() else None
        folder = owning_animation(self.art.layers, node.id) if node else None
        item = self.timeline.currentItem()
        cel_id = item.data(Qt.UserRole) if item is not None else None
        if folder is None or not cel_id:
            return
        with self._edit("the timeline"):
            if not move_cel(folder, cel_id, delta):
                return
            self._sync_animations()
        self._refresh_timeline()
        self._select_layer(cel_id)
        self.recomposite()
        self._save_session()
        self.status.showMessage("Moved the frame on the timeline. The layer stack stayed put.")

    def mark_current_animation(self):
        index = self.tree.currentIndex()
        node = self.model.node_of(index) if index.isValid() else None
        self.mark_animation(node)

    def mark_animation(self, node):
        if not self.art or node is None or node.kind != "group":
            self.status.showMessage("Pick a folder to mark as an animation.")
            return
        with self._edit("the animation"):
            result = set_animation(self.art.layers, node.id, not node.animation)
        if result is not None and result.id != node.id:
            self.status.showMessage("%s is a frame of %s." % (node.name, result.name))
            return
        self._select_layer(node.id)
        self._sync_animations()
        self.tree.viewport().update()
        self._refresh_timeline()
        self.recomposite()
        self._save_session()
        if node.animation:
            self.status.showMessage("%s is an animation. Each folder inside it is one full-canvas frame." % node.name)
        else:
            self.status.showMessage("%s is a plain folder." % node.name)

    def _sync_animations(self):
        """Keep a named animation object on its timeline. Do not invent a name."""
        if not self.art:
            return
        changed = False
        for obj in self.objects:
            if not obj.origin.startswith("folder:"):
                continue
            folder = find_node(self.art.layers, obj.origin.split(":", 1)[1])
            if folder is None or not folder.animation:
                continue
            ids, assign = frames_of(folder)
            obj.layer_ids = ids
            obj.assign = assign
            changed = True
        if changed:
            current = self._current_object()
            self._fill_objects(select=current.id if current else None)

    def toggle_visible(self, node):
        if not self.art or node is None:
            return
        with self._edit("visibility"):
            set_visible(self.art.layers, node.id, not node.visible)
        self.recomposite()
        self._show_slots()
        self._save_session()
        self.status.showMessage("%s %s." % ("Showing" if node.visible else "Hidden", node.name))

    def toggle_mute(self, node):
        if not self.art or node is None:
            return
        with self._edit("mute"):
            set_mute(self.art.layers, node.id, not node.mute)
        self.tree.viewport().update()
        self.status.showMessage("%s %s." % ("Muted" if node.mute else "Unmuted", node.name))
        self.recomposite()
        self._save_soon()

    def toggle_solo(self, node):
        if not self.art or node is None:
            return
        with self._edit("solo"):
            set_solo(self.art.layers, node.id, not node.solo)
        self.tree.viewport().update()
        last = not node.solo and not any(item.solo for item in walk(self.art.layers))
        self.recomposite(REST_WAIT if last else None)
        if last and self.picture.is_busy():
            self.status.showMessage(REST_WAIT)
        elif node.solo:
            self.status.showMessage("Solo %s. The other tracks are out of the picture." % node.name)
        elif any(item.solo for item in walk(self.art.layers)):
            self.status.showMessage("Solo off for %s." % node.name)
        else:
            self.status.showMessage("Solo off. Every track is back in the picture.")
        self._save_soon()

    def toggle_clip(self, node):
        if not self.art or node is None:
            return
        with self._edit("the clip"):
            set_clip(self.art.layers, node.id, not node.clip)
        self.tree.viewport().update()
        if node.clip:
            self.status.showMessage("%s clips to the layer below." % node.name)
        else:
            self.status.showMessage("%s does not clip." % node.name)
        self.recomposite()
        self._save_soon()

    def begin_opacity(self, node, y):
        if not self.art or node is None:
            return
        self._opacity_before = capture(self)
        self._opacity_id = node.id
        self._opacity_y = int(y)
        self._opacity_start = clamp_opacity(node.opacity)
        self.status.showMessage("%s opacity is %02X." % (node.name, self._opacity_start))

    def drag_opacity(self, y):
        if not self.art or not getattr(self, "_opacity_id", None):
            return
        node = find_node(self.art.layers, self._opacity_id)
        if node is None:
            return
        # Up raises the value. A hundred pixels covers the whole range, as on a tracker dial.
        delta = self._opacity_y - int(y)
        value = clamp_opacity(self._opacity_start + round(delta * 255 / 100.0))
        if clamp_opacity(node.opacity) == value:
            return
        node.opacity = value
        self.tree.viewport().update()
        self.status.showMessage("%s opacity is %02X." % (node.name, value))
        self.recomposite()

    def end_opacity(self):
        before = getattr(self, "_opacity_before", None)
        self._opacity_before = None
        self._opacity_id = None
        if before is None:
            return
        self.history.remember(self, "the opacity", before)
        self._save_soon()

    def _save_soon(self):
        """Mute and solo write the session after the click, not on every repeat."""
        if self.art:
            self._save_timer.start()

    def choose_blend(self, node, global_pos):
        """Open the blend list for this row. The click does not change the selection."""
        if not self.art or node is None:
            return
        menu = QMenu(self)
        current = blend_label(node.blend, getattr(node, "shapes", True))
        actions = {}
        marked = None
        for key, label in BLEND_CHOICES:
            action = menu.addAction(label)
            action.setCheckable(True)
            if label == current:
                action.setChecked(True)
                marked = action
            actions[action] = key
        if marked is not None:
            menu.setActiveAction(marked)
        chosen = menu.exec(global_pos)
        if chosen not in actions:
            return
        with self._edit("the blend"):
            assign_blend(node, actions[chosen])
        shown = blend_label(node.blend, getattr(node, "shapes", True))
        self.status.showMessage("%s blend is %s." % (node.name, shown))
        self.tree.viewport().update()
        self.recomposite()
        self._save_session()

    def toggle_pick(self, node):
        if node is None:
            return
        if node.id in self.picked:
            self.picked.discard(node.id)
        else:
            self.picked.add(node.id)
        self.tree.viewport().update()
        self._save_session()

    def eventFilter(self, watched, event):
        kind = event.type()
        if kind in (
            QEvent.Type.TouchBegin,
            QEvent.Type.TouchUpdate,
            QEvent.Type.TouchEnd,
            QEvent.Type.TouchCancel,
        ) and self.isVisible():
            if self._take_fingers(event):
                return True
        if kind == QEvent.Type.KeyPress and self.isVisible():
            if self._save_key(event):
                return True
            if not self._text_focus() and self._history_key(event):
                return True
            if not self._text_focus() and self._cut_key(event):
                return True
        return super().eventFilter(watched, event)

    def _save_key(self, event):
        if QApplication.activeModalWidget() is not None or event.isAutoRepeat():
            return False
        mods = event.modifiers()
        if event.key() != Qt.Key_S or not (mods & Qt.ControlModifier):
            return False
        if mods & (Qt.ShiftModifier | Qt.AltModifier | Qt.MetaModifier):
            return False
        self.save_blueprint()
        return True

    def _history_key(self, event):
        if QApplication.activeModalWidget() is not None or event.isAutoRepeat():
            return False
        mods = event.modifiers()
        if event.key() != Qt.Key_Z or not (mods & Qt.ControlModifier):
            return False
        if mods & (Qt.AltModifier | Qt.MetaModifier):
            return False
        if mods & Qt.ShiftModifier:
            self.redo()
        else:
            self.undo()
        return True

    def _take_fingers(self, event):
        if event.type() == QEvent.Type.TouchCancel:
            self._fingers.reset()
            return False
        try:
            points = event.points()
        except Exception:
            return False
        rows = []
        released = QEventPoint.State.Released
        for point in points:
            kind = point.device().pointerType() if point.device() is not None else None
            pens = {QPointingDevice.PointerType.Pen}
            eraser = getattr(QPointingDevice.PointerType, "Eraser", None)
            if eraser is not None:
                pens.add(eraser)
            pos = point.position()
            rows.append((point.id(), pos.x(), pos.y(), point.state() != released, kind not in pens))
        action = self._fingers.update(rows, int(time.monotonic() * 1000))
        if self._fingers.max_count >= 2 and getattr(self, "picture", None) is not None:
            self.picture._drag = None
        if action == "undo":
            self.undo()
            return True
        if action == "redo":
            self.redo()
            return True
        return False

    def _edit(self, label):
        return self.history.editing(self, label)

    def undo(self):
        label = self.history.undo(self)
        if label is None:
            self.status.showMessage("Nothing to undo.")
            return
        self._after_history()
        self.status.showMessage("Undid %s." % label)

    def redo(self):
        label = self.history.redo(self)
        if label is None:
            self.status.showMessage("Nothing to redo.")
            return
        self._after_history()
        self.status.showMessage("Redid %s." % label)

    def _after_history(self):
        if not self.art:
            return
        self.picked = {ident for ident in self.picked if find_node(self.art.layers, ident)}
        reset_plate_cache()
        self.model.set_layers(self.art.layers)
        self._show_playlist()
        self._fill_objects(keep=True)
        self._refresh_timeline()
        self.tree.viewport().update()
        self.recomposite()
        self._save_session()

    def _cut_key(self, event):
        """Shift+X, and the cut keys, even when the picture has focus."""
        if QApplication.activeModalWidget() is not None:
            return False
        mods = event.modifiers()
        if mods & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier):
            return False
        key = event.key()
        if key == Qt.Key_X and (mods & Qt.ShiftModifier) and not event.isAutoRepeat():
            self.toggle_cut()
            return True
        if not self.picture.cut_active:
            return False
        if key == Qt.Key_Escape and not event.isAutoRepeat():
            if self.picture._draft or self.picture.selection:
                self.picture.clear_selection()
                self.status.showMessage("Selection cleared.")
            else:
                self.toggle_cut()
            return True
        if key in (Qt.Key_Return, Qt.Key_Enter) and self.picture.cut_tool == "poly":
            if self.picture.close_polygon():
                self.status.showMessage("Selection closed.")
            return True
        if key == Qt.Key_Backspace and self.picture.cut_tool == "poly" and self.picture._draft:
            self.picture.pop_point()
            return True
        if key == Qt.Key_Delete and not event.isAutoRepeat():
            self.delete_selection()
            return True
        return False

    def keyPressEvent(self, event):
        if self._cut_key(event):
            event.accept()
            return
        if not self._text_focus() and not (event.modifiers() & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)):
            if event.key() in (Qt.Key_Plus, Qt.Key_Equal):
                self.picture.zoom_by(ZOOM_STEP)
                event.accept()
                return
            if event.key() == Qt.Key_Minus:
                self.picture.zoom_by(1.0 / ZOOM_STEP)
                event.accept()
                return
        super().keyPressEvent(event)

    def _cut_bar(self):
        bar = QWidget()
        row = QHBoxLayout(bar)
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(4)
        bar.setStyleSheet(
            "QPushButton { background: #000000; color: #3a3a3a; border: 1px solid #2a2a2a;"
            " border-radius: 0; padding: 4px 8px; }"
            "QPushButton:hover { color: #ffffff; border-color: #ffffff; }"
            "QPushButton#cutOn { background: #ffffff; color: #000000; border-color: #ffffff; }"
        )
        self.cut_tools = {}
        for key, label in (("rect", "Rect"), ("lasso", "Lasso"), ("poly", "Polygon")):
            button = QPushButton(label)
            button.clicked.connect(lambda _checked=False, name=key: self._set_cut_tool(name))
            self.cut_tools[key] = button
            row.addWidget(button)
        cut = QPushButton("Cut to folder")
        cut.clicked.connect(self.cut_selection)
        clear = QPushButton("Delete")
        clear.clicked.connect(self.delete_selection)
        done = QPushButton("Done")
        done.clicked.connect(self.toggle_cut)
        row.addWidget(cut)
        row.addWidget(clear)
        row.addWidget(done)
        bar.hide()
        return bar

    def _set_cut_tool(self, name):
        self.picture.cut_tool = name
        self.picture._draft = []
        self.picture._hover = None
        self.picture.update()
        for key, button in self.cut_tools.items():
            button.setObjectName("cutOn" if key == name else "")
            button.style().unpolish(button)
            button.style().polish(button)
        if name == "poly":
            self.status.showMessage("Polygon: click each corner. Enter or double-click closes it.")
        elif name == "lasso":
            self.status.showMessage("Lasso: drag around the shape.")
        else:
            self.status.showMessage("Rect: drag a rectangle.")

    def _cut_folder_node(self):
        if not self.art:
            return None
        current = None
        index = self.tree.currentIndex()
        if index.isValid():
            current = self.model.node_of(index)
        if current is None and len(self.picked) == 1:
            current = find_node(self.art.layers, next(iter(self.picked)))
        if current is None:
            return None
        if current.kind == "group":
            return current
        return parent_of(self.art.layers, current.id)

    def toggle_cut(self):
        if self._text_focus():
            return
        if self.picture.cut_active:
            self.picture.set_cut(False)
            self.cut_bar.hide()
            self._cut_target = None
            self.status.showMessage("Cut mode off.")
            return
        folder = self._cut_folder_node()
        if folder is None or folder.kind != "group":
            self.status.showMessage("Select a folder. Shift+X cuts every layer inside it.")
            return
        self._cut_target = folder.id
        self.picture.cut_tool = "rect"
        self.picture.set_cut(True)
        self._set_cut_tool("rect")
        self.cut_bar.show()
        self.status.showMessage(
            "Cut mode on %s. Draw a selection. It touches every layer in the folder." % folder.name
        )

    def cut_selection(self):
        if not self._cut_ready():
            return
        dialog = CutFolderDialog(self, next_cut_name(self.art.layers))
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        name = dialog.name()
        if not name:
            self.status.showMessage("Type a name before the folder is cut.")
            return
        make_object = dialog.wants_object()
        if make_object and any(obj.name.lower() == name.lower() for obj in self.objects):
            self.status.showMessage("There is already an object named %s." % name)
            return
        self._apply_selection(False, name, make_object)

    def delete_selection(self):
        self._apply_selection(True)

    def _cut_ready(self):
        if not self.art or not self.picture.cut_active:
            return False
        if len(self.picture.selection) < 3:
            self.status.showMessage("Draw a selection first.")
            return False
        folder = find_node(self.art.layers, self._cut_target)
        if folder is None or folder.kind != "group":
            self.status.showMessage("Select a folder. Shift+X cuts every layer inside it.")
            return False
        return True

    def _apply_selection(self, erase, name="", make_object=False):
        if not self._cut_ready():
            return
        points = self.picture.selection
        folder = find_node(self.art.layers, self._cut_target)
        with self._edit("the delete" if erase else "the cut"):
            if erase:
                count = erase_folder(self.art.layers, folder.id, points, self.art.width, self.art.height)
                if not count:
                    self.status.showMessage("Nothing in that selection.")
                    return
                message = "Cleared %d layers in %s. The drawing file is unchanged." % (count, folder.name)
                focus = folder.id
            else:
                made, count = cut_folder(
                    self.art.layers, folder.id, points, self.art.width, self.art.height, name
                )
                if made is None:
                    self.status.showMessage("Nothing in that selection.")
                    return
                message = "Cut %d layers into %s. The drawing file is unchanged." % (count, made.name)
                focus = made.id
                if make_object:
                    layers = [layer for layer in paint_layers([made]) if layer.kind == "layer"]
                    obj = DeskObject(
                        _next_object_id(), made.name, [layer.id for layer in layers],
                        {layer.id: (0, 0) for layer in layers},
                        origin="static:%s" % made.id, explicit=True,
                    )
                    obj.stack = object_z(self.art, obj)
                    self.objects.append(obj)
                    self._fill_objects(select=obj.id)
                    message = (
                        "Cut %d layers into %s. %s is in the object list. The drawing file is unchanged."
                        % (count, made.name, made.name)
                    )
            reset_plate_cache()
            self.model.set_layers(self.art.layers)
            chosen = find_node(self.art.layers, focus)
            if chosen is not None:
                self.select_only(chosen)
            self.picture.clear_selection()
            self.recomposite()
            self._save_session()
            self.status.showMessage(message)

    def _zoom_picture(self, factor):
        if self._text_focus():
            return
        self.picture.zoom_by(factor)

    def _text_focus(self):
        widget = QApplication.focusWidget()
        return isinstance(widget, (QLineEdit, QSpinBox, QPlainTextEdit))

    def _focus_id(self):
        index = self.tree.currentIndex()
        if not index.isValid():
            return None
        node = self.model.node_of(index)
        return node.id if node is not None else None

    def choose_picture(self, x, y):
        """Highlight the layer painted at this document pixel."""
        if not self.art or x is None or y is None:
            self.status.showMessage("No layer there.")
            return None
        ident = pick_layer(self.art.width, self.art.height, self.art.layers, x, y, self._focus_id())
        node = find_node(self.art.layers, ident) if ident else None
        if node is None:
            self.status.showMessage("No layer there.")
            return None
        self.select_only(node)
        self.status.showMessage(node.name)
        return node

    def select_only(self, node):
        self.picked.clear()
        if node is not None:
            self.picked.add(node.id)
            self.tree._anchor = node.id
            self._select_layer(node.id)
        self.tree.viewport().update()
        self._save_session()

    def select_range(self, anchor_id, target_id):
        ordered = [item.id for item in self.tree.visible_nodes()]
        chosen = range_ids(ordered, anchor_id, target_id)
        if not chosen and target_id:
            chosen = [target_id]
        self.picked.clear()
        self.picked.update(chosen)
        self.tree.viewport().update()
        self._select_layer(target_id)
        self._save_session()

    def group_selection(self):
        """Clip Studio's Create folder and insert layer. One folder, the selection, once."""
        if not self.art or self.tree._editor is not None:
            return
        ids = [ident for ident in self.picked if find_node(self.art.layers, ident)]
        if not ids:
            current = self.model.node_of(self.tree.currentIndex()) if self.tree.currentIndex().isValid() else None
            if current is None:
                self.status.showMessage("Select layers, then Ctrl+G.")
                return
            ids = [current.id]
        with self._edit("the group"):
            layers, focus = new_folder(self.art.layers, None, ids)
            self.art.layers = layers
            self.model.set_layers(layers)
            self.picked.clear()
            self.picked.add(focus)
            self.tree._anchor = focus
        self.recomposite()
        self._select_layer(focus)
        self._save_session()
        count = len(ids)
        self.status.showMessage("Grouped %d layer%s." % (count, "" if count == 1 else "s"))

    def rename_layer(self, ident, name):
        if not self.art:
            return
        with self._edit("the rename"):
            rename_node(self.art.layers, ident, name)
            mark_warps(self.art.layers)
        self.tree.viewport().update()
        self._fill_objects(keep=True)
        self._save_session()

    def make_folder(self, node):
        if not self.art or node is None:
            return
        with self._edit("the folder"):
            layers, focus = new_folder(self.art.layers, node.id, list(self.picked))
            self.art.layers = layers
            self.model.set_layers(layers)
        self.recomposite()
        self._select_layer(focus)
        self._save_session()

    def arrange_layers(self, ids, target_id, place):
        if not self.art:
            return
        if self.arrange_locked:
            self.status.showMessage("Unlock the layers to rearrange them.")
            return
        with self._edit("the move"):
            moved = arrange(self.art.layers, ids, target_id, place)
            if moved:
                layers, focus = moved
                self.art.layers = layers
                self._sync_animations()
        if not moved:
            self.status.showMessage("Those layers cannot move there.")
            return
        layers, focus = moved
        self.picked = {ident for ident in self.picked if find_node(layers, ident)}
        if not self.picked and focus:
            self.picked.add(focus)
        self.model.set_layers(layers)
        self.recomposite()
        self._select_layer(focus)
        self._save_session()
        count = len(ids)
        self.status.showMessage("Moved %d layer%s." % (count, "" if count == 1 else "s"))

    def move_picked(self, node):
        if not self.art or node is None:
            return
        if self.arrange_locked:
            self.status.showMessage("Unlock the layers to rearrange them.")
            return
        with self._edit("the move"):
            moved = move_into(self.art.layers, list(self.picked), node.id)
            if moved:
                layers, focus = moved
                self.art.layers = layers
        if not moved:
            self.status.showMessage("Those layers cannot move into %s." % node.name)
            return
        layers, focus = moved
        self.model.set_layers(layers)
        self.recomposite()
        self._select_layer(focus)
        self._save_session()

    def _select_layer(self, ident):
        node = find_node(self.art.layers, ident) if self.art else None
        if node is None:
            return
        for ancestor in self.model.ancestors(node):
            parent = self.model.index_for(ancestor)
            if parent.isValid():
                self.tree.expand(parent)
        index = self.model.index_for(node)
        if index.isValid():
            self.tree.setCurrentIndex(index)
            self.tree.scrollTo(index)

    def _picked_layers(self):
        layers = []
        seen = set()
        for ident in self.picked:
            node = find_node(self.art.layers, ident) if self.art else None
            if node is None:
                continue
            chosen = paint_layers([node]) if node.kind == "group" else [node]
            for layer in chosen:
                if layer.kind == "layer" and layer.id not in seen:
                    seen.add(layer.id)
                    layers.append(layer)
        return layers

    def make_object(self):
        if not self.art:
            return
        layers = self._picked_layers()
        if not layers:
            self.status.showMessage("Pick layers first.")
            return
        dialog = NameDialog(self, [layer.name for layer in layers])
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        self._create_named_object(dialog.name(), layers)

    def _create_named_object(self, name, layers):
        name = (name or "").strip()
        if not name:
            self.status.showMessage("Type a name before the object is created.")
            return None
        if any(obj.name.lower() == name.lower() for obj in self.objects):
            self.status.showMessage("There is already an object named %s." % name)
            return None
        obj = DeskObject(_next_object_id(), name, [layer.id for layer in layers], explicit=True)
        obj.stack = object_z(self.art, obj)
        owners = []
        for layer in layers:
            owner = owning_animation(self.art.layers, layer.id)
            if owner is None or (owners and owner.id != owners[0].id):
                owners = []
                break
            owners = [owner]
        if owners:
            ids, assign = frames_of(owners[0])
            if set(ids) == {layer.id for layer in layers}:
                obj.layer_ids = ids
                obj.assign = assign
                obj.origin = "folder:%s" % owners[0].id
        with self._edit("the object"):
            self.objects.append(obj)
        self._fill_objects(select=obj.id)
        self._save_session()
        self.status.showMessage("Made %s from %d layers." % (name, len(obj.layer_ids)))
        return obj

    def delete_objects(self):
        if not self.art:
            return
        chosen = []
        for item in self.objects_list.selectedItems():
            ident = item.data(0, Qt.UserRole)
            if ident and ident not in chosen:
                chosen.append(ident)
        if not chosen:
            current = self._current_object()
            if current is not None:
                chosen = [current.id]
        if not chosen:
            self.status.showMessage("Pick an object to delete.")
            return
        with self._edit("the object delete"):
            gone = set(chosen)
            by_id = {obj.id: obj for obj in self.objects}
            for obj in self.objects:
                if obj.id in gone:
                    continue
                parent = obj.parent
                while parent in gone:
                    parent = by_id[parent].parent if parent in by_id else ""
                obj.parent = "" if parent in gone else parent
            self.objects = [obj for obj in self.objects if obj.id not in gone]
        self._fill_objects()
        self._save_session()
        count = len(gone)
        self.status.showMessage("Deleted %d object%s." % (count, "" if count == 1 else "s"))

    def find_frames(self):
        obj = self._current_object()
        if not self.art or obj is None:
            self.status.showMessage("Pick an object first.")
            return
        extra = extra_frames(self.art, obj)
        if not extra:
            self.status.showMessage("No other frames of %s." % obj.name)
            return
        have = set(obj.layer_ids)
        with self._edit("the frames"):
            for layer in extra:
                if layer.id not in have:
                    obj.layer_ids.append(layer.id)
                    have.add(layer.id)
        self._show_slots()
        self._save_session()
        self.status.showMessage("Found %d of %s." % (len(extra), obj.name))

    def assign_frame(self):
        if not self.art:
            return
        ids = []
        for ident in self.picked:
            node = find_node(self.art.layers, ident)
            if node and node.kind == "layer":
                ids.append(node.id)
        obj = self._current_object()
        if not ids and obj is not None:
            ids = list(obj.layer_ids)
        if not ids:
            self.status.showMessage("Pick the layers to assign.")
            return
        ids = visual_ids(self.art.layers, ids)
        first = find_node(self.art.layers, ids[0])
        state, frame = 0, 0
        if obj is not None and first and first.id in obj.assign:
            state, frame = obj.assign[first.id]
        if obj is None:
            layers = [find_node(self.art.layers, ident) for ident in ids]
            layers = [layer for layer in layers if layer]
            naming = NameDialog(self, [layer.name for layer in layers])
            if naming.exec() != QDialog.DialogCode.Accepted:
                return
            obj = self._create_named_object(naming.name(), layers)
            if obj is None:
                return
        result = AssignDialog.ask(self, len(ids), state, frame)
        if result is None:
            return
        state, frame, mode = result
        ordered = [find_node(self.art.layers, ident) for ident in ids]
        ordered = [layer for layer in ordered if layer]
        with self._edit("the assign"):
            replacement = apply_frames(obj, ids, state, frame, mode, ordered)
            self.objects = [replacement if item.id == obj.id else item for item in self.objects]
        self._fill_objects(select=replacement.id)
        self._save_session()
        self.status.showMessage("Assigned %s." % slot_key(state, frame))

    def prepare_warp_object(self, node):
        """One named object for this warp target system. The drawing stays the source."""
        if not self.art or not warp_system_folder(node):
            self.status.showMessage("That folder is not a warp target system.")
            return
        layers = warp_system_layers(node)
        if not layers:
            self.status.showMessage("%s has no warp targets." % node.name)
            return
        dialog = NameDialog(self, [layer.name for layer in layers], node.name)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        name = dialog.name()
        if not name:
            self.status.showMessage("Type a name before the object is created.")
            return
        if any(obj.name.lower() == name.lower() for obj in self.objects):
            self.status.showMessage("There is already an object named %s." % name)
            return
        obj = DeskObject(
            _next_object_id(), name, [layer.id for layer in layers],
            {layer.id: (0, 0) for layer in layers},
            origin="warp:%s" % node.id, explicit=True, role="target",
            kind="warp target", warp_layer=warp_layer_kind(layers),
        )
        obj.stack = object_z(self.art, obj)
        with self._edit("the warp object"):
            self.objects.append(obj)
        self._fill_objects(select=obj.id)
        self._save_session()
        self.status.showMessage("Prepared %s as a warp target." % name)

    def create_folder_object(self, node):
        """Name this folder as one object. The pictures are written later, on Export."""
        if not self.art or not static_asset_folder(node):
            self.status.showMessage("That folder is not one static picture.")
            return
        layers = static_layers(node)
        dialog = NameDialog(self, [layer.name for layer in layers], node.name)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        name = dialog.name()
        if not name:
            self.status.showMessage("Type a name before the object is created.")
            return
        if any(obj.name.lower() == name.lower() for obj in self.objects):
            self.status.showMessage("There is already an object named %s." % name)
            return
        obj = DeskObject(
            _next_object_id(), name, [layer.id for layer in layers],
            {layer.id: (0, 0) for layer in layers},
            origin="static:%s" % node.id, explicit=True,
        )
        obj.stack = object_z(self.art, obj)
        with self._edit("the folder object"):
            self.objects.append(obj)
        self._fill_objects(select=obj.id)
        self._save_session()
        self.status.showMessage("Made %s. It is in the object list." % name)

    def export_static_folder(self, node):
        """One folder, one 000000.PNG, written without the other objects."""
        if not self.art or not static_asset_folder(node):
            self.status.showMessage("That folder is not one static picture.")
            return
        layers = static_layers(node)
        dialog = NameDialog(self, [layer.name for layer in layers], node.name)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        name = dialog.name()
        if not name:
            self.status.showMessage("Type a name before the object is created.")
            return
        if any(obj.name.lower() == name.lower() for obj in self.objects):
            self.status.showMessage("There is already an object named %s." % name)
            return
        start = os.path.dirname(self.art.path) or os.path.expanduser("~")
        parent = QFileDialog.getExistingDirectory(self, "Export static folder", start)
        if not parent:
            return
        obj = DeskObject(
            _next_object_id(), name, [layer.id for layer in layers],
            {layer.id: (0, 0) for layer in layers},
            origin="static:%s" % node.id, explicit=True,
        )
        obj.stack = object_z(self.art, obj)
        if not self._write_objects(parent, [obj]):
            return
        with self._edit("the export object"):
            self.objects.append(obj)
        self._fill_objects(select=obj.id)
        self._save_session()
        self.status.showMessage("Exported %s as 000000.PNG." % name)

    def object_from_subfolders(self, node):
        """A new object. Each subfolder becomes the next slot position."""
        if not self.art or not static_asset_folder(node):
            self.status.showMessage("That folder is not one static picture.")
            return
        layers, assign = slots_from_subfolders(node)
        groups = [
            child for child in node.children
            if child.kind == "group" and not child.omit and child.visible and static_layers(child)
        ]
        if not groups:
            self.status.showMessage("%s has no subfolders." % node.name)
            return
        dialog = NameDialog(self, [layer.name for layer in layers], node.name)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        name = dialog.name()
        if not name:
            self.status.showMessage("Type a name before the object is created.")
            return
        if any(obj.name.lower() == name.lower() for obj in self.objects):
            self.status.showMessage("There is already an object named %s." % name)
            return
        obj = DeskObject(
            _next_object_id(), name, [layer.id for layer in layers], assign,
            origin="slots:%s" % node.id, explicit=True,
        )
        obj.stack = object_z(self.art, obj)
        with self._edit("the slot object"):
            self.objects.append(obj)
        self._fill_objects(select=obj.id)
        self._save_session()
        keys = []
        for layer in layers:
            key = slot_key(*assign[layer.id])
            if key not in keys:
                keys.append(key)
        self.status.showMessage("Made %s. Positions %s." % (name, ", ".join(keys)))

    def _write_objects(self, parent, objects):
        scene = _safe_folder(plan_scene(self.art, objects)["scene"])
        dest = os.path.join(parent, scene)
        if os.path.isdir(dest) and os.listdir(dest):
            answer = QMessageBox.question(self, "VMI STUDIO", "Replace the folder %s?" % dest)
            if answer != QMessageBox.StandardButton.Yes:
                return False
            shutil.rmtree(dest)
        try:
            write_scene(parent, self.art, objects, self.playlist)
        except Exception as exc:
            self.status.showMessage(str(exc))
            return False
        return True

    def export_scene(self):
        if not self.art:
            self.status.showMessage("Open a drawing first.")
            return
        start = os.path.dirname(self.art.path) or os.path.expanduser("~")
        parent = QFileDialog.getExistingDirectory(self, "Export scene folder", start)
        if not parent:
            return
        scene = _safe_folder(plan_scene(self.art, self.objects)["scene"])
        dest = os.path.join(parent, scene)
        if os.path.isdir(dest) and os.listdir(dest):
            answer = QMessageBox.question(self, "VMI STUDIO", "Replace the folder %s?" % dest)
            if answer != QMessageBox.StandardButton.Yes:
                return
            shutil.rmtree(dest)
        try:
            result = write_scene(parent, self.art, self.objects, self.playlist)
        except Exception as exc:
            QMessageBox.warning(self, "VMI STUDIO", str(exc))
            return
        self.status.showMessage("Wrote %d pictures to %s" % (result["pictures"], result["root"]))

    def _fill_objects(self, select=None, keep=False):
        current = self._current_object()
        if keep and select is None and current is not None:
            select = current.id
        self.objects_list.blockSignals(True)
        self.objects_list.clear()
        ids = {obj.id for obj in self.objects}
        children = {}
        roots = []
        for obj in self.objects:
            parent = obj.parent if obj.parent in ids and obj.parent != obj.id else ""
            if parent:
                children.setdefault(parent, []).append(obj)
            else:
                roots.append(obj)
        chosen = []

        def add(obj, parent_item):
            item = QTreeWidgetItem([obj.name])
            item.setData(0, Qt.UserRole, obj.id)
            item.setData(0, Qt.UserRole + 1, object_role(self.art, obj))
            item.setFlags(
                item.flags()
                | Qt.ItemIsEditable
                | Qt.ItemIsEnabled
                | Qt.ItemIsSelectable
                | Qt.ItemIsDragEnabled
                | Qt.ItemIsDropEnabled
            )
            if parent_item is None:
                self.objects_list.addTopLevelItem(item)
            else:
                parent_item.addChild(item)
            if select and obj.id == select:
                chosen.append(item)
            kids = sorted(children.get(obj.id, []), key=lambda row: (-int(row.stack or 0), row.id))
            for kid in kids:
                add(kid, item)

        for obj in sorted(roots, key=lambda row: (-int(row.stack or 0), row.id)):
            add(obj, None)
        self.objects_list.expandAll()
        self.objects_list.blockSignals(False)
        if chosen:
            self.objects_list.setCurrentItem(chosen[0])
        elif self.objects_list.topLevelItemCount():
            self.objects_list.setCurrentItem(self.objects_list.topLevelItem(0))
        self._show_slots()

    def _apply_object_tree(self):
        with self._edit("the object stack"):
            arrange_objects(self.objects, self.objects_list.rows())
        self._fill_objects(keep=True)
        self._save_session()
        self.status.showMessage("Object stack saved. The top of each group is in front.")

    def _current_object(self):
        item = self.objects_list.currentItem()
        if item is None:
            return None
        ident = item.data(0, Qt.UserRole)
        for obj in self.objects:
            if obj.id == ident:
                return obj
        return None

    def _object_layers(self, obj):
        found = []
        if not self.art or obj is None:
            return found
        for ident in obj.layer_ids:
            layer = find_node(self.art.layers, ident)
            if layer is not None:
                found.append(layer)
        return found

    def _set_choice(self, box, value):
        index = box.findData(value or "")
        box.setCurrentIndex(0 if index < 0 else index)

    def _show_playlist(self):
        self._field_guard = True
        self._set_choice(self.playlist_box, self.playlist)
        self._field_guard = False

    def _show_fields(self):
        self._field_guard = True
        obj = self._current_object()
        enabled = obj is not None and self.art is not None
        self.object_name.setEnabled(enabled)
        self.object_kind.setEnabled(enabled)
        self.object_warp.setEnabled(enabled)
        self.object_sound.setEnabled(enabled)
        if not enabled:
            self.object_name.setText("")
            self.object_kind.setCurrentIndex(0)
            self.object_warp.setCurrentIndex(0)
            self.object_sound.setCurrentIndex(0)
            self.kind_note.setText("Pick an object.")
            self._field_guard = False
            return
        self.object_name.setText(obj.name)
        self._set_choice(self.object_kind, obj.kind)
        self._set_choice(self.object_warp, obj.warp_layer)
        self._set_choice(self.object_sound, obj.sound)
        self.kind_note.setText("Exports as %s." % assigned_type(self.art, obj))
        self._field_guard = False

    def _playlist_chosen(self, index):
        if self._field_guard:
            return
        with self._edit("the playlist"):
            self.playlist = clean_playlist(self.playlist_box.itemData(index))
        if self.playlist == "menu":
            self.status.showMessage("This level plays the menu playlist.")
        elif self.playlist == "battle":
            self.status.showMessage("This level plays the battle playlist.")
        else:
            self.status.showMessage("This level leaves the Webamp playlist off.")
        if self.art:
            self._save_session()

    def _name_edited(self):
        if self._field_guard:
            return
        obj = self._current_object()
        if obj is None:
            return
        name = self.object_name.text().strip()
        if name == obj.name:
            return
        if not name:
            self.status.showMessage("Type a name before it is applied.")
            self._show_fields()
            return
        if any(other.id != obj.id and other.name.lower() == name.lower() for other in self.objects):
            self.status.showMessage("There is already an object named %s." % name)
            self._show_fields()
            return
        with self._edit("the name"):
            obj.name = name
        self._fill_objects(select=obj.id)
        self._save_session()
        self.status.showMessage("Named %s." % name)

    def _kind_chosen(self, index):
        if self._field_guard:
            return
        obj = self._current_object()
        if obj is None:
            return
        kind = clean_kind(self.object_kind.itemData(index))
        if kind == (obj.kind or ""):
            return
        with self._edit("the type"):
            obj.kind = kind
            if kind == "warp target":
                if obj.role != "window":
                    obj.role = "target"
                if not clean_warp_layer(obj.warp_layer):
                    obj.warp_layer = warp_layer_kind(self._object_layers(obj))
            elif obj.role == "target":
                obj.role = ""
        self._fill_objects(select=obj.id)
        self._save_session()
        self.status.showMessage("%s is %s." % (obj.name, assigned_type(self.art, obj)))

    def _warp_chosen(self, index):
        if self._field_guard:
            return
        obj = self._current_object()
        if obj is None:
            return
        with self._edit("the warp layer"):
            obj.warp_layer = clean_warp_layer(self.object_warp.itemData(index))
        self._save_session()
        if obj.warp_layer:
            self.status.showMessage("%s warp layer is a %s." % (obj.name, obj.warp_layer))
        else:
            self.status.showMessage("%s has no warp layer." % obj.name)

    def _sound_chosen(self, index):
        if self._field_guard:
            return
        obj = self._current_object()
        if obj is None:
            return
        with self._edit("the sound"):
            obj.sound = clean_sound(self.object_sound.itemData(index))
        self._save_session()
        if obj.sound:
            self.status.showMessage("%s triggers %s." % (obj.name, self.object_sound.itemText(index)))
        else:
            self.status.showMessage("%s triggers no sound." % obj.name)

    def _show_slots(self):
        self.slots.blockSignals(True)
        self.slots.clear()
        obj = self._current_object()
        rows = slot_rows(self.art, obj) if self.art and obj is not None else [
            {"key": slot_key(state, 0), "label": state_label(state), "layers": []}
            for state in (0, 1, 2, 3)
        ]
        editable = obj is not None
        for row in rows:
            item = QTreeWidgetItem([
                row["key"],
                row["label"],
                ", ".join(row["layers"]),
            ])
            item.setData(0, Qt.UserRole, row["key"])
            flags = Qt.ItemIsEnabled | Qt.ItemIsSelectable
            if editable:
                flags |= Qt.ItemIsEditable
            item.setFlags(flags)
            self.slots.addTopLevelItem(item)
        self.slots.blockSignals(False)
        if self.slots.columnWidth(0) < 70:
            self.slots.setColumnWidth(0, 72)
            self.slots.setColumnWidth(1, 90)
        self._show_fields()

    def _slot_renamed(self, item, column):
        if column != 1 or not self.art:
            return
        obj = self._current_object()
        key = item.data(0, Qt.UserRole) if item is not None else None
        if obj is None or not key:
            return
        text = item.text(1).strip()
        with self._edit("the label"):
            if text:
                obj.labels[key] = text
            else:
                obj.labels.pop(key, None)
        self._save_session()
        self._show_slots()

    def add_slot(self):
        obj = self._current_object()
        if not self.art or obj is None:
            self.status.showMessage("Make an object first, then name its positions.")
            return
        dialog = SlotNameDialog(self)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return
        name, state, frame = dialog.values()
        if not name:
            self.status.showMessage("Type a name for the position.")
            return
        key = slot_key(state, frame)
        with self._edit("the slot"):
            obj.labels[key] = name
        self._show_slots()
        self._save_session()
        self.status.showMessage("Named %s as %s." % (key, name))

    def _object_renamed(self, item, _column=0):
        ident = item.data(0, Qt.UserRole)
        name = item.text(0).strip()
        for obj in self.objects:
            if obj.id != ident:
                continue
            if not name or any(
                other.id != obj.id and other.name.lower() == name.lower() for other in self.objects
            ):
                if name and name.lower() != obj.name.lower():
                    self.status.showMessage("There is already an object named %s." % name)
                self.objects_list.blockSignals(True)
                item.setText(0, obj.name)
                self.objects_list.blockSignals(False)
                self._show_fields()
                return
            if name == obj.name:
                return
            with self._edit("the name"):
                obj.name = name
            break
        self._fill_objects(select=ident)
        self._save_session()
        self.status.showMessage("Named %s." % name)

    def _object_menu(self, pos):
        item = self.objects_list.itemAt(pos)
        if item is None:
            return
        self.objects_list.setCurrentItem(item)
        menu = QMenu(self)
        rename = menu.addAction("Rename")
        delete = menu.addAction("Delete")
        chosen = menu.exec(self.objects_list.mapToGlobal(pos))
        if chosen == rename:
            self.objects_list.editItem(item)
        elif chosen == delete:
            self.delete_objects()

    def _save_session(self):
        timer = getattr(self, "_save_timer", None)
        if timer is not None:
            timer.stop()
        if not self.art:
            return
        save({
            "path": self.art.path,
            "signature": signature(self.art.layers),
            "visible": visibility_map(self.art.layers),
            "picked": list(self.picked),
            "selected": self.model.data(self.tree.currentIndex(), Qt.UserRole) if self.tree.currentIndex().isValid() else "",
            "splitter": self.split.sizes(),
            "objects": [
                {
                    "id": obj.id,
                    "name": obj.name,
                    "layer_ids": list(obj.layer_ids),
                    "assign": {ident: [pos[0], pos[1]] for ident, pos in obj.assign.items()},
                    "origin": obj.origin,
                    "labels": dict(obj.labels),
                    "explicit": bool(obj.explicit),
                    "parent": obj.parent or "",
                    "stack": int(obj.stack or 0),
                    "role": obj.role or "",
                    "kind": obj.kind or "",
                    "warp_layer": obj.warp_layer or "",
                    "sound": obj.sound or "",
                }
                for obj in self.objects
            ],
            "animations": [
                {"id": node.id, "timeline": list(node.timeline)}
                for node in walk(self.art.layers)
                if node.animation
            ],
            "font": self._font_id,
            "arrange_locked": bool(self.arrange_locked),
            "mute": track_ids(self.art.layers, "mute"),
            "solo": track_ids(self.art.layers, "solo"),
            "clips": track_ids(self.art.layers, "clip"),
            "blends": blend_rows(self.art.layers),
            "split_version": SPLIT_VERSION,
            "tree_width": int(self.tree.tree_width),
            "playlist": self.playlist or "",
        })

    def closeEvent(self, event):
        self._picture_pending = False
        self._picture_generation += 1
        job = self._picture_job
        if self._job_alive() and job is not None:
            job.wait(1500)
        self._save_session()
        if self._loader is not None and self._loader.isRunning():
            self._loader.wait(1500)
        super().closeEvent(event)

    def _snapshot(self):
        here = os.path.dirname(os.path.abspath(__file__))
        found = []
        for name in os.listdir(here):
            if not name.endswith((".py", ".c")):
                continue
            path = os.path.join(here, name)
            try:
                found.append((name, os.path.getmtime(path), os.path.getsize(path)))
            except OSError:
                continue
        return tuple(sorted(found))

    def _watch(self):
        if self._loading:
            return
        now = self._snapshot()
        if now == self._watch_base:
            self._watch_pending = None
            return
        if now != self._watch_pending:
            self._watch_pending = now
            return
        self._save_session()
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        os.chdir(root)
        current = os.environ.get("PYTHONPATH", "")
        parts = current.split(os.pathsep) if current else []
        if root not in parts:
            os.environ["PYTHONPATH"] = root if not current else root + os.pathsep + current
        argv = [sys.executable, "-m", "vmi_studio", "--dev"]
        if self.art and self.art.path:
            argv.append(self.art.path)
        os.execv(sys.executable, argv)


def _pixmap(image):
    data = image.tobytes("raw", "RGBA")
    qimg = QImage(data, image.width, image.height, image.width * 4, QImage.Format.Format_RGBA8888).copy()
    return QPixmap.fromImage(qimg)


def _saved_font_id():
    data = load() or {}
    if isinstance(data, dict):
        return data.get("font") or fonts.DEFAULT_ID
    return fonts.DEFAULT_ID


def apply_style(app, font_id=None):
    app.setStyle("Fusion")
    requested = _saved_font_id() if font_id is None else font_id
    try:
        face, family = fonts.activate(requested)
    except Exception:
        face, family = fonts.activate(fonts.DEFAULT_ID)
    _paint_font(app, face, family)
    return face.id


def _paint_font(app, face, family):
    font = QFont(family)
    font.setPixelSize(12)
    if face.category == "pixel":
        font.setStyleStrategy(QFont.StyleStrategy.NoAntialias)
        font.setHintingPreference(QFont.HintingPreference.PreferNoHinting)
    else:
        if face.category == "monospace" or face.id == fonts.DEFAULT_ID:
            font.setStyleHint(QFont.StyleHint.Monospace)
        font.setHintingPreference(QFont.HintingPreference.PreferDefaultHinting)
    app.setFont(font)
    safe = family.replace("\\", "").replace('"', "")
    app.setStyleSheet(STYLESHEET % safe)


STYLESHEET = """
QWidget { background: #000000; color: #3a3a3a; font-size: 12px; }
QMenuBar, QTreeView, QTreeWidget, QListWidget, QPlainTextEdit, QPushButton, QLineEdit, QSpinBox, QComboBox, QScrollArea, QLabel, QStatusBar, QDialog, QMessageBox, QHeaderView::section { font-family: "%s"; }
QLabel#title { color: #2a2a2a; }
QMenuBar { background: #000000; color: #3a3a3a; border-bottom: 1px solid #2a2a2a; }
QMenuBar::item:selected { background: #111111; color: #ffffff; }
QMenu { background: #000000; color: #3a3a3a; border: 1px solid #2a2a2a; }
QMenu::item:selected { background: #111111; color: #ffffff; }
QTreeView, QTreeWidget, QListWidget, QPlainTextEdit { background: #000000; color: #3a3a3a; border: none; outline: none; }
QTreeView::item:selected, QTreeWidget::item:selected, QListWidget::item:selected { background: #111111; color: #ffffff; }
QHeaderView::section { background: #000000; color: #2a2a2a; border: none; border-bottom: 1px solid #2a2a2a; padding: 2px 4px; }
QSplitter::handle { background: #141414; }
QSplitter::handle:hover { background: #3a3a3a; }
QSplitter::handle:pressed { background: #ffffff; }
QPushButton { background: #000000; color: #3a3a3a; border: 1px solid #2a2a2a; padding: 4px 8px; border-radius: 0; }
QPushButton:hover { color: #ffffff; border-color: #ffffff; }
QPushButton:pressed, QPushButton[current="true"] { color: #ffffff; border-color: #ffffff; }
QPushButton#export { background: #ffffff; color: #000000; border: 1px solid #ffffff; }
QPushButton#export:pressed { background: #ffffff; color: #000000; }
QLineEdit, QSpinBox { background: #000000; color: #ffffff; border: 1px solid #2a2a2a; border-radius: 0; padding: 2px 4px; selection-background-color: #ffffff; selection-color: #000000; }
QComboBox { background: #000000; color: #3a3a3a; border: 1px solid #2a2a2a; border-radius: 0; padding: 2px 6px; }
QComboBox:focus, QComboBox:on { color: #ffffff; border-color: #ffffff; }
QComboBox::drop-down { border: none; width: 16px; }
QComboBox QAbstractItemView { background: #000000; color: #3a3a3a; border: 1px solid #2a2a2a; selection-background-color: #111111; selection-color: #ffffff; outline: 0; }
QScrollArea { background: #000000; border: none; }
QTreeWidget::item { padding: 0px; border: none; }
QStatusBar { background: #000000; color: #3a3a3a; border-top: 1px solid #2a2a2a; }
QScrollBar:vertical { background: #000000; width: 10px; margin: 0; }
QScrollBar::handle:vertical { background: #2a2a2a; min-height: 24px; }
QScrollBar::handle:vertical:hover { background: #3a3a3a; }
QScrollBar:horizontal { background: #000000; height: 10px; margin: 0; }
QScrollBar::handle:horizontal { background: #2a2a2a; min-width: 24px; }
QScrollBar::handle:horizontal:hover { background: #3a3a3a; }
QScrollBar::add-line, QScrollBar::sub-line { height: 0; width: 0; }
QScrollBar::add-page, QScrollBar::sub-page { background: #000000; }
QMessageBox { background: #000000; color: #3a3a3a; }
"""


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if "--version" in argv:
        from vmi_studio import __version__

        print("VMI STUDIO %s" % __version__)
        return 0
    if "--help" in argv or "-h" in argv:
        from vmi_studio import __version__

        print(
            "VMI STUDIO %s\n"
            "Open a layered drawing and export one folder per object.\n\n"
            "  vmi-studio [file.clip|psd|psb|kra|xcf|vmib] [--dev]\n\n"
            "--dev  restarts when the app code changes and reopens the same drawing.\n"
            % __version__
        )
        return 0
    faulthandler.enable()
    dev = "--dev" in argv
    args = [arg for arg in argv if arg != "--dev"]
    path = args[0] if args else None
    app = QApplication.instance() or QApplication(sys.argv)
    apply_style(app)
    window = MainWindow(dev=dev)
    window.show()
    if path:
        window.open_path(path)
    return app.exec()
