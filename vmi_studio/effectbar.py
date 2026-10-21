"""KOSV parameters for one layer.

The rows are that layer's stack. Names and ranges come from the KOSV catalog.
The bar writes the layer. The desk repaints.
"""

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QDoubleSpinBox,
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

from vmi_studio.kosv import CATALOG, STACKS, _PARAM, _at, make_stack


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


def _shown(name, params):
    if isinstance(params, dict) and name in params:
        return _at({"params": params}, name, 0)
    return _PARAM[name][2]


class _BarScroll(QScrollArea):
    """A strip that does not widen the picture pane."""

    def __init__(self, cap=0):
        super().__init__()
        self._cap = int(cap)
        self.setFrameShape(QFrame.NoFrame)
        self.setWidgetResizable(True)
        self.setMinimumWidth(0)
        self.setMinimumHeight(0)
        self.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        if cap:
            self.setVerticalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        else:
            self.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)

    def minimumSizeHint(self):
        return QSize(0, 0)

    def sizeHint(self):
        widget = self.widget()
        if widget is None:
            return QSize(0, 0)
        height = widget.sizeHint().height()
        if self._cap:
            height = min(height, self._cap)
        return QSize(0, height)


class EffectBar(QWidget):
    """One layer's KOSV stack. editing fires before a write. changed fires after it."""

    editing = Signal()
    changed = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("effectBar")
        self.setMinimumWidth(0)
        self.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
        self._node = None
        self._guard = False
        self.preset_buttons = {}
        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(4)

        self.preset_host = QWidget()
        presets = QHBoxLayout(self.preset_host)
        presets.setContentsMargins(0, 0, 0, 0)
        presets.setSpacing(4)
        for name in STACKS:
            button = QPushButton(name)
            button.setObjectName("kosvPreset")
            button.setCursor(Qt.PointingHandCursor)
            button.clicked.connect(lambda _checked=False, preset=name: self._use_preset(preset))
            self.preset_buttons[name] = button
            presets.addWidget(button)
        self.clear_button = QPushButton("Clear")
        self.clear_button.setObjectName("kosvClear")
        self.clear_button.setCursor(Qt.PointingHandCursor)
        self.clear_button.clicked.connect(self._clear)
        presets.addWidget(self.clear_button)
        presets.addStretch(1)
        self.preset_scroll = _BarScroll()
        self.preset_scroll.setWidget(self.preset_host)
        root.addWidget(self.preset_scroll)

        self.stack_body = QWidget()
        self.stack_body.setObjectName("kosvStack")
        stack = QVBoxLayout(self.stack_body)
        stack.setContentsMargins(0, 0, 0, 0)
        stack.setSpacing(4)
        self.stack_scroll = _BarScroll(cap=120)
        self.stack_scroll.setWidget(self.stack_body)
        root.addWidget(self.stack_scroll)
        self._reload()

    def minimumSizeHint(self):
        return self.sizeHint()

    def sizeHint(self):
        preset = getattr(self, "preset_scroll", None)
        stack = getattr(self, "stack_scroll", None)
        height = preset.sizeHint().height() if preset is not None else 24
        if stack is not None and stack.isVisible():
            layout = self.layout()
            spacing = layout.spacing() if layout is not None else 0
            height += spacing + stack.sizeHint().height()
        return QSize(0, max(height, 24))

    def bind(self, node):
        """Show this layer. None leaves the presets with nothing to write."""
        self._node = node
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
        column.addWidget(check)

        host = QWidget()
        host.setObjectName("kosvParams")
        params_row = QHBoxLayout(host)
        params_row.setContentsMargins(0, 0, 0, 0)
        params_row.setSpacing(4)
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
            spin.setValue(_shown(name, params))
            spin.setToolTip(name)
            spin.valueChanged.connect(
                lambda value, index=index, name=name: self._param_changed(index, name, value)
            )
            params_row.addWidget(label)
            params_row.addWidget(spin)
        params_row.addStretch(1)
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
        if isinstance(current, (int, float)) and not isinstance(current, bool):
            if abs(float(current) - number) < 1e-9:
                return
        self.editing.emit()
        params[name] = number
        self.changed.emit()

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
