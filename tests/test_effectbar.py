"""The KOSV bar writes one layer's stack. It does not paint."""

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtWidgets import QApplication, QCheckBox, QDoubleSpinBox, QLabel, QPushButton

from vmi_studio.document import ArtFile, Node, refresh
from vmi_studio.effectbar import EffectBar
from vmi_studio.kosv import CATALOG, STACKS, effect_ids, make_effect, make_stack


def _ids(bar):
    found = []
    layout = bar.stack_body.layout()
    for index in range(layout.count()):
        row = layout.itemAt(index).widget()
        if row is None:
            continue
        box = row.findChild(QCheckBox)
        found.append(box.text())
    return tuple(found)


def _params(bar):
    found = []
    layout = bar.stack_body.layout()
    for index in range(layout.count()):
        row = layout.itemAt(index).widget()
        if row is None:
            continue
        host = None
        for child in row.findChildren(QLabel):
            parent = child.parentWidget()
            if parent is not None and parent.objectName() == "kosvParams":
                host = parent
                break
        if host is None:
            continue
        row_names = []
        strip = host.layout()
        for slot in range(strip.count()):
            widget = strip.itemAt(slot).widget()
            if isinstance(widget, QLabel):
                row_names.append(widget.text())
        found.append(tuple(row_names))
    return found


def _spin(bar, ident, name):
    layout = bar.stack_body.layout()
    for index in range(layout.count()):
        row = layout.itemAt(index).widget()
        box = row.findChild(QCheckBox)
        if box is None or box.text() != ident:
            continue
        host = None
        for child in row.findChildren(QLabel):
            parent = child.parentWidget()
            if parent is not None and parent.objectName() == "kosvParams":
                host = parent
                break
        strip = host.layout()
        for slot in range(strip.count()):
            widget = strip.itemAt(slot).widget()
            if isinstance(widget, QLabel) and widget.text() == name:
                nxt = strip.itemAt(slot + 1).widget()
                return nxt
    raise AssertionError("%s %s" % (ident, name))


class EffectBarTests(unittest.TestCase):
    def setUp(self):
        self.app = QApplication.instance() or QApplication([])

    def _bar(self, effects):
        node = Node("n1", "door", "layer")
        node.effects = effects
        bar = EffectBar()
        bar.bind(node)
        return node, bar

    def test_catalog_ids_shown_match_the_engine(self):
        effects = [make_effect(ident) for ident in effect_ids()]
        node, bar = self._bar([])
        bar.apply_to(node)
        node.effects = effects
        bar.bind(node)
        self.assertEqual(_ids(bar), effect_ids())
        expected = [tuple(names) for _ident, _label, names in CATALOG]
        self.assertEqual(_params(bar), expected)
        labels = []
        strip = bar.preset_host.layout()
        for index in range(strip.count()):
            widget = strip.itemAt(index).widget()
            if isinstance(widget, QPushButton) and widget is not bar.clear_button:
                labels.append(widget.text())
        self.assertEqual(labels, list(STACKS))

    def test_enabled_toggle_sticks_on_the_node(self):
        node, bar = self._bar([make_effect("vhs", {"tear": 0.2})])
        effect = node.effects[0]
        params = effect["params"]
        box = bar.stack_body.layout().itemAt(0).widget().findChild(QCheckBox)
        self.assertTrue(box.isChecked())
        box.setChecked(False)
        self.assertFalse(effect["enabled"])
        self.assertIs(node.effects[0], effect)
        self.assertIs(effect["params"], params)
        self.assertEqual(params["tear"], 0.2)
        bar.bind(node)
        box = bar.stack_body.layout().itemAt(0).widget().findChild(QCheckBox)
        self.assertFalse(box.isChecked())
        self.assertFalse(node.effects[0]["enabled"])
        box.setChecked(True)
        self.assertTrue(node.effects[0]["enabled"])

    def test_clear_empties_effects(self):
        node, bar = self._bar([make_effect(ident) for ident in effect_ids()])
        same = node.effects
        bar.clear_button.click()
        self.assertIs(node.effects, same)
        self.assertEqual(node.effects, [])
        self.assertEqual(_ids(bar), ())

    def test_a_param_is_written_in_place(self):
        node, bar = self._bar([make_effect("fisheye", {"spread": 0.02})])
        effect = node.effects[0]
        params = effect["params"]
        same = node.effects
        _spin(bar, "fisheye", "amount").setValue(0.8)
        self.assertIs(node.effects, same)
        self.assertIs(node.effects[0], effect)
        self.assertIs(effect["params"], params)
        self.assertAlmostEqual(params["amount"], 0.8)
        self.assertAlmostEqual(params["spread"], 0.02)

    def test_a_preset_uses_make_stack(self):
        node, bar = self._bar([make_effect("vhs")])
        same = node.effects
        bar.preset_buttons["glow"].click()
        self.assertIs(node.effects, same)
        self.assertEqual(node.effects, make_stack("glow"))
        self.assertEqual(_ids(bar), tuple(effect["id"] for effect in make_stack("glow")))

    def test_the_desk_recomposites_when_a_value_changes(self):
        import vmi_studio.window as window_mod

        real_save, real_load = window_mod.save, window_mod.load
        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        window_mod.apply_style(self.app)
        win = None
        try:
            win = window_mod.MainWindow()
            layout = win.draw_bar.parentWidget().layout()
            self.assertGreater(layout.indexOf(win.effect_bar), layout.indexOf(win.draw_bar))
            self.assertLess(layout.indexOf(win.effect_bar), layout.indexOf(win.cut_bar))
            node = Node("n1", "door", "layer")
            node.effects = [make_effect("vhs", {"tear": 0.2})]
            win.art = ArtFile("room.clip", "", "clip", 4, 4, refresh([node]), "")
            win.model.set_layers(win.art.layers)
            win._select_layer(node.id)
            self.assertEqual(_ids(win.effect_bar), ("vhs",))
            seen = []
            win.recomposite = lambda *args, **kwargs: seen.append(True)
            box = win.effect_bar.stack_body.layout().itemAt(0).widget().findChild(QCheckBox)
            box.setChecked(False)
            self.assertFalse(node.effects[0]["enabled"])
            self.assertTrue(seen)
            win.undo()
            self.assertTrue(node.effects[0]["enabled"])
        finally:
            if win is not None:
                win.close()
            window_mod.save = real_save
            window_mod.load = real_load
