"""One open drawing, one export folder. Tabs do not share an output path."""

import os

from vmi_studio.desk import os_stem
from vmi_studio.naming import folder_name
from vmi_studio.sceneio import _safe_folder


def scene_dirname(file_name):
    """The scene folder write_scene creates under the tab's export parent."""
    return _safe_folder(folder_name(os_stem(file_name or "")))


def default_export_parent(path):
    """A folder that belongs to this drawing and no other path."""
    path = os.path.abspath(path)
    parent = os.path.dirname(path)
    return os.path.join(parent, scene_dirname(os.path.basename(path)) + "-vmi")


def export_root(parent, file_name):
    parent_abs = os.path.abspath(parent)
    root = os.path.join(parent_abs, scene_dirname(file_name))
    return os.path.abspath(root)


def same_export(parent_a, name_a, parent_b, name_b):
    if not parent_a or not parent_b:
        return False
    return export_root(parent_a, name_a) == export_root(parent_b, name_b)
