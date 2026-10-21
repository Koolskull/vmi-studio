"""On and off marks for the desk.

A hollow square is off. A filled square with a check is on. The field flips
with the mark: white when on, black when off.
"""

from PySide6.QtCore import QSize, Qt
from PySide6.QtGui import QColor, QIcon, QImage, QPainter, QPen, QPixmap

_ON = (
    "QPushButton { background: #ffffff; color: #000000; border: 1px solid #ffffff;"
    " border-radius: 0; padding: 4px 8px; }"
)
_OFF = (
    "QPushButton { background: #000000; color: #ffffff; border: 1px solid #2a2a2a;"
    " border-radius: 0; padding: 4px 8px; }"
)
_ON_COMPACT = (
    "QPushButton { background: #ffffff; color: #000000; border: 1px solid #ffffff;"
    " border-radius: 0; padding: 0 6px; }"
)
_OFF_COMPACT = (
    "QPushButton { background: #000000; color: #ffffff; border: 1px solid #2a2a2a;"
    " border-radius: 0; padding: 0 6px; }"
)


def toggle_icon(on, ink="#ffffff"):
    """A 16×16 mark. The check is the opposite of the fill so it stays readable."""
    image = QImage(16, 16, QImage.Format.Format_ARGB32)
    image.fill(QColor(0, 0, 0, 0))
    color = QColor(ink)
    painter = QPainter(image)
    painter.setRenderHint(QPainter.Antialiasing, False)
    painter.setPen(QPen(color))
    painter.setBrush(color if on else Qt.NoBrush)
    painter.drawRect(2, 2, 11, 11)
    if on:
        opposite = QColor("#000000") if color.lightness() > 128 else QColor("#ffffff")
        painter.setPen(QPen(opposite))
        painter.drawLine(5, 8, 7, 11)
        painter.drawLine(7, 11, 11, 5)
    painter.end()
    return QIcon(QPixmap.fromImage(image))


def show_toggle(button, on=None, compact=False):
    """Paint one button for the state it is in now."""
    if on is None:
        on = bool(button.isChecked())
    on = bool(on)
    button.setIcon(toggle_icon(on, "#000000" if on else "#ffffff"))
    button.setIconSize(QSize(16, 16))
    if compact:
        button.setStyleSheet(_ON_COMPACT if on else _OFF_COMPACT)
    else:
        button.setStyleSheet(_ON if on else _OFF)
    return button


def watch(button):
    """Keep a checkable button's icon and field matched to isChecked."""
    button.setCheckable(True)

    def refresh(*_args):
        show_toggle(button)

    button.toggled.connect(refresh)
    show_toggle(button)
    return button


def watch_check(box):
    """A checkbox whose mark is the icon. The theme box is hidden."""
    box.setIconSize(QSize(16, 16))

    def refresh(*_args):
        on = bool(box.isChecked())
        box.setIcon(toggle_icon(on, "#ffffff" if on else "#3a3a3a"))
        box.setStyleSheet(
            "QCheckBox { color: %s; spacing: 6px; background: transparent; }"
            "QCheckBox::indicator { width: 0px; height: 0px; border: none; }"
            % ("#ffffff" if on else "#3a3a3a")
        )

    box.toggled.connect(refresh)
    refresh()
    return box
