"""KOSV parameters for one layer.

The rows are that layer's stack. Names and ranges come from the KOSV catalog.
The bar writes the layer. The desk repaints.
"""

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QDoubleSpinBox,
    QFrame,
    QGridLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

from vmi_studio.kosv import CATALOG, STACKS, _PARAM, _at, make_stack
from vmi_studio.toggles import watch_check


_NAMES = {ident: names for ident, _label, names in CATALOG}
_LABELS = {ident: label for ident, label, _names in CATALOG}


def _decimals(low, high):
    if float(high) - float(low) >= 32.0:
        return 2
    return 4


def _step(low, high):
    span = float(high) - float(low)
    if span >= 32.0:
        return 1.0
    if span > 2.0:
        return 0.1
    return 0.01


def _shown(name, params, frame=0):
    if isinstance(params, dict) and name in params:
        return _at({"params": params}, name, frame)
    return _PARAM[name][2]


def _upsert_key(params, name, frame, value):
    """Hold keys. A plain number becomes a key at frame 0, then this frame."""
    raw = params.get(name)
    keys = []
    if isinstance(raw, dict):
        for row in raw.get("keys") or []:
            if isinstance(row, dict) and "frame" in row:
                keys.append({"frame": int(row["frame"]), "value": float(row["value"])})
    elif isinstance(raw, (int, float)) and not isinstance(raw, bool):
        keys.append({"frame": 0, "value": float(raw)})
    frame = int(frame)
    number = float(value)
    found = False
    for row in keys:
        if row["frame"] == frame:
            row["value"] = number
            found = True
    if not found:
        keys.append({"frame": frame, "value": number})
    keys.sort(key=lambda row: row["frame"])
    params[name] = {"keys": keys}


class _BarScroll(QScrollArea):
    """The stack list. It fills the effects column and scrolls inside it."""

    def __init__(self):
        super().__init__()
        self.setFrameShape(QFrame.NoFrame)
        self.setWidgetResizable(True)
        self.setMinimumWidth(0)
        self.setMinimumHeight(0)
        self.setSizePolicy(QSizePolicy.Preferred, QSizePolicy.Expanding)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarAsNeeded)

    def minimumSizeHint(self):
        return QSize(0, 0)

    def sizeHint(self):
        return QSize(180, 160)


class EffectBar(QWidget):
    """The layer effects column. editing fires before a write. changed fires after it.

    A folder is a valid target. Its stack runs after the layers inside it.
    """

    editing = Signal()
    changed = Signal()
    load_plate = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("effectBar")
        self.setMinimumWidth(168)
        self.setMaximumWidth(220)
        self.setSizePolicy(QSizePolicy.Preferred, QSizePolicy.Expanding)
        self._node = None
        self._guard = False
        self._frame = 0
        self._spins = []
        self._last_spin = None
        self.preset_buttons = {}
        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(4)

        self.heading = QLabel("EFFECTS")
        self.heading.setObjectName("title")
        self.bound_name = QLabel("No layer")
        self.bound_name.setObjectName("effectTarget")
        self.bound_name.setWordWrap(True)
        root.addWidget(self.heading)
        root.addWidget(self.bound_name)

        self.load_button = QPushButton("Load plate")
        self.load_button.setObjectName("kosvLoad")
        self.load_button.setCursor(Qt.PointingHandCursor)
        self.load_button.setToolTip("A picture, or one frame of a video. A folder's stack covers it with the layers inside.")
        self.load_button.clicked.connect(self.load_plate.emit)
        root.addWidget(self.load_button)

        self.preset_host = QWidget()
        presets = QGridLayout(self.preset_host)
        presets.setContentsMargins(0, 0, 0, 0)
        presets.setSpacing(4)
        row = 0
        col = 0
        for name in STACKS:
            button = QPushButton(name)
            button.setObjectName("kosvPreset")
            button.setCursor(Qt.PointingHandCursor)
            button.clicked.connect(lambda _checked=False, preset=name: self._use_preset(preset))
            self.preset_buttons[name] = button
            presets.addWidget(button, row, col)
            col += 1
            if col == 2:
                col = 0
                row += 1
        self.clear_button = QPushButton("Clear")
        self.clear_button.setObjectName("kosvClear")
        self.clear_button.setCursor(Qt.PointingHandCursor)
        self.clear_button.clicked.connect(self._clear)
        presets.addWidget(self.clear_button, row, col)
        root.addWidget(self.preset_host)

        self.stack_body = QWidget()
        self.stack_body.setObjectName("kosvStack")
        stack = QVBoxLayout(self.stack_body)
        stack.setContentsMargins(0, 0, 0, 0)
        stack.setSpacing(4)
        self.stack_scroll = _BarScroll()
        self.stack_scroll.setWidget(self.stack_body)
        root.addWidget(self.stack_scroll, 1)
        self._reload()

    def minimumSizeHint(self):
        return QSize(168, 0)

    def sizeHint(self):
        return QSize(200, 280)

    def bind(self, node):
        """Show this layer or folder. None leaves the presets with nothing to write."""
        self._node = node
        if node is None:
            self.bound_name.setText("No layer")
        else:
            kind = "folder" if getattr(node, "kind", "") == "group" else "layer"
            self.bound_name.setText("%s  %s" % (node.name, kind))
        self._reload()

    def apply_to(self, node):
        self.bind(node)

    def _reload(self):
        self._guard = True
        try:
            self._clear_rows()
            node = self._node
            alive = node is not None
            self.clear_button.setEnabled(alive)
            for button in self.preset_buttons.values():
                button.setEnabled(alive)
            effects = []
            if alive:
                raw = getattr(node, "effects", None)
                if isinstance(raw, list):
                    effects = raw
            for index, effect in enumerate(effects):
                if not isinstance(effect, dict):
                    continue
                ident = effect.get("id")
                names = _NAMES.get(ident)
                if not names:
                    continue
                self._add_row(index, ident, names, effect)
            self.stack_scroll.setVisible(self.stack_body.layout().count() > 0)
        finally:
            self._guard = False
        self.updateGeometry()

    def _clear_rows(self):
        self._spins = []
        layout = self.stack_body.layout()
        while layout.count():
            item = layout.takeAt(0)
            widget = item.widget()
            if widget is None:
                continue
            # Stay parented until delete. A null parent would become a window.
            widget.hide()
            widget.deleteLater()

    def _add_row(self, index, ident, names, effect):
        row = QWidget()
        row.setObjectName("kosvRow")
        column = QVBoxLayout(row)
        column.setContentsMargins(0, 0, 0, 0)
        column.setSpacing(2)
        check = QCheckBox(ident)
        check.setObjectName("kosvEffect")
        check.setToolTip(_LABELS.get(ident, ident))
        check.setChecked(bool(effect.get("enabled", True)))
        check.toggled.connect(lambda on, index=index: self._enabled_changed(index, on))
        watch_check(check)
        column.addWidget(check)

        host = QWidget()
        host.setObjectName("kosvParams")
        params_col = QVBoxLayout(host)
        params_col.setContentsMargins(0, 0, 0, 0)
        params_col.setSpacing(2)
        params = effect.get("params") if isinstance(effect.get("params"), dict) else {}
        for name in names:
            low, high, _default = _PARAM[name]
            label = QLabel(name)
            label.setObjectName("kosvParam")
            spin = QDoubleSpinBox()
            spin.setRange(float(low), float(high))
            spin.setDecimals(_decimals(low, high))
            spin.setSingleStep(_step(low, high))
            spin.setKeyboardTracking(False)
            spin.setValue(_shown(name, params, self._frame))
            spin.setToolTip(name)
            spin.valueChanged.connect(
                lambda value, index=index, name=name: self._param_changed(index, name, value)
            )
            self._spins.append((index, name, spin))
            params_col.addWidget(label)
            params_col.addWidget(spin)
        column.addWidget(host)
        self.stack_body.layout().addWidget(row)

    def _enabled_changed(self, index, on):
        if self._guard or self._node is None:
            return
        effects = getattr(self._node, "effects", None)
        if not isinstance(effects, list) or index < 0 or index >= len(effects):
            return
        effect = effects[index]
        if not isinstance(effect, dict):
            return
        if bool(effect.get("enabled", True)) == bool(on):
            return
        self.editing.emit()
        effect["enabled"] = bool(on)
        self.changed.emit()

    def _param_changed(self, index, name, value):
        if self._guard or self._node is None:
            return
        effects = getattr(self._node, "effects", None)
        if not isinstance(effects, list) or index < 0 or index >= len(effects):
            return
        effect = effects[index]
        if not isinstance(effect, dict):
            return
        params = effect.get("params")
        if not isinstance(params, dict):
            params = {}
            effect["params"] = params
        number = float(value)
        current = params.get(name)
        self._last_spin = (index, name)
        if isinstance(current, dict) and isinstance(current.get("keys"), list):
            shown = _at(effect, name, self._frame)
            if abs(float(shown) - number) < 1e-9:
                return
            self.editing.emit()
            _upsert_key(params, name, self._frame, number)
            self.changed.emit()
            return
        if isinstance(current, (int, float)) and not isinstance(current, bool):
            if abs(float(current) - number) < 1e-9:
                return
        self.editing.emit()
        params[name] = number
        self.changed.emit()

    def show_frame(self, frame):
        """Move the spins to this playhead. Keyed settings change. Constants do not."""
        self._frame = int(frame)
        if self._guard or self._node is None:
            return
        effects = getattr(self._node, "effects", None)
        if not isinstance(effects, list):
            return
        self._guard = True
        try:
            for index, name, spin in self._spins:
                if index < 0 or index >= len(effects):
                    continue
                effect = effects[index]
                if not isinstance(effect, dict):
                    continue
                params = effect.get("params") if isinstance(effect.get("params"), dict) else {}
                spin.setValue(_shown(name, params, self._frame))
        finally:
            self._guard = False

    def insert_key(self, frame):
        """Shift+I. Record the focused setting, or the last one, at this frame."""
        if self._node is None or not self._spins:
            return False
        from PySide6.QtWidgets import QApplication

        focus = QApplication.focusWidget()
        target = None
        for index, name, spin in self._spins:
            if spin is focus:
                target = (index, name, spin)
                break
        if target is None and self._last_spin is not None:
            for index, name, spin in self._spins:
                if (index, name) == self._last_spin:
                    target = (index, name, spin)
                    break
        if target is None:
            target = self._spins[0]
        index, name, spin = target
        effects = getattr(self._node, "effects", None)
        if not isinstance(effects, list) or index < 0 or index >= len(effects):
            return False
        effect = effects[index]
        if not isinstance(effect, dict):
            return False
        params = effect.get("params")
        if not isinstance(params, dict):
            params = {}
            effect["params"] = params
        self._frame = int(frame)
        self.editing.emit()
        _upsert_key(params, name, frame, spin.value())
        self._last_spin = (index, name)
        self.changed.emit()
        return True

    def _use_preset(self, name):
        if self._guard or self._node is None:
            return
        stack = make_stack(name)
        self.editing.emit()
        effects = getattr(self._node, "effects", None)
        if isinstance(effects, list):
            effects[:] = stack
        else:
            self._node.effects = list(stack)
        self._reload()
        self.changed.emit()

    def _clear(self):
        if self._guard or self._node is None:
            return
        current = getattr(self._node, "effects", None)
        if isinstance(current, list) and not current:
            return
        self.editing.emit()
        if isinstance(current, list):
            current.clear()
        else:
            self._node.effects = []
        self._reload()
        self.changed.emit()
