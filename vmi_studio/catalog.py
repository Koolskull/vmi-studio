"""Named project and task lists. Nothing here reads a drawing."""

import json
import os
import time


PATH = os.path.join(os.path.expanduser("~"), ".cache", "vmi-studio", "launcher.json")
DRAWING_EXTS = (".clip", ".psd", ".psb", ".kra", ".xcf", ".vmib")
IMAGE_EXTS = (".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".tif", ".tiff", ".tga")
STYLES = ("project", "task")
RECENT_LIMIT = 12


def user_name():
    for key in ("USER", "LOGNAME"):
        value = os.environ.get(key)
        if value:
            return value
    return "user"


def is_drawing(path):
    return os.path.splitext(path or "")[1].lower() in DRAWING_EXTS


def is_image(path):
    return os.path.splitext(path or "")[1].lower() in IMAGE_EXTS


def empty():
    return {"recent": [], "lists": [], "browse": "", "seq": 0}


def _title(value):
    return " ".join(str(value or "").split())


def _take_id(book, raw_id, used):
    ident = str(raw_id or "")
    if not ident or ident in used:
        book["seq"] = int(book.get("seq") or 0) + 1
        ident = "l%d" % book["seq"]
        while ident in used:
            book["seq"] += 1
            ident = "l%d" % book["seq"]
    used.add(ident)
    return ident


def _clean_items(raw_items):
    items = []
    paths = set()
    for item in raw_items or []:
        if not isinstance(item, dict):
            continue
        path = item.get("path")
        if not isinstance(path, str) or not path:
            continue
        path = os.path.abspath(path)
        if path in paths:
            continue
        paths.add(path)
        items.append({
            "path": path,
            "label": _title(item.get("label")) or os.path.basename(path),
            "tag": _title(item.get("tag")),
            "done": bool(item.get("done")),
        })
    return items


def _has_loose_tasks(raw):
    if not isinstance(raw, dict):
        return False
    for item in raw.get("lists") or []:
        if isinstance(item, dict) and item.get("style") == "task":
            return True
    return False


def _walk_lists(book):
    for item in book.get("lists") or []:
        yield item
        for task in item.get("tasks") or []:
            yield task


def _place_loose_task(book, projects, raw, used):
    title = _title(raw.get("title"))
    if not title or not projects:
        return
    items = _clean_items(raw.get("items"))
    owners = {}
    for project in projects:
        for item in project.get("items") or []:
            owners.setdefault(item["path"], project["id"])
    grouped = {project["id"]: [] for project in projects}
    homeless = []
    for item in items:
        owner = owners.get(item["path"])
        if owner is None:
            homeless.append(item)
        else:
            grouped[owner].append(item)
    if not items:
        _append_task(book, projects[0], title, [], used, raw.get("id"))
        return
    if homeless:
        ranked = sorted(projects, key=lambda project: len(grouped[project["id"]]), reverse=True)
        target = ranked[0]
        have = {item["path"] for item in target.get("items") or []}
        for item in homeless:
            if item["path"] not in have:
                target.setdefault("items", []).append(dict(item))
                have.add(item["path"])
            grouped[target["id"]].append(item)
    first = True
    for project in projects:
        rows = grouped[project["id"]]
        if not rows:
            continue
        _append_task(book, project, title, rows, used, raw.get("id") if first else "")
        first = False


def _append_task(book, project, title, items, used, raw_id):
    project.setdefault("tasks", []).append({
        "id": _take_id(book, raw_id, used),
        "title": title,
        "style": "task",
        "project": project["id"],
        "items": items,
    })


def normalize(data):
    book = empty()
    if not isinstance(data, dict):
        return book
    try:
        book["seq"] = int(data.get("seq") or 0)
    except (TypeError, ValueError):
        book["seq"] = 0
    browse = data.get("browse") or ""
    book["browse"] = browse if isinstance(browse, str) else ""
    seen = set()
    for raw in data.get("recent") or []:
        if not isinstance(raw, dict):
            continue
        path = raw.get("path")
        if not isinstance(path, str) or not path:
            continue
        path = os.path.abspath(path)
        if path in seen:
            continue
        seen.add(path)
        opened = raw.get("opened") if isinstance(raw.get("opened"), str) else ""
        tag = " ".join(str(raw.get("tag") or "").split()) or user_name()
        book["recent"].append({
            "path": path,
            "opened": opened,
            "done": bool(raw.get("done")),
            "tag": tag,
        })
    book["recent"] = book["recent"][:RECENT_LIMIT]
    used = set()
    projects = []
    loose = []
    for raw in data.get("lists") or []:
        if not isinstance(raw, dict):
            continue
        style = raw.get("style")
        title = _title(raw.get("title"))
        if style not in STYLES or not title:
            continue
        if style == "task":
            loose.append(raw)
            continue
        ident = _take_id(book, raw.get("id"), used)
        tasks = []
        for task in raw.get("tasks") or []:
            if not isinstance(task, dict):
                continue
            task_title = _title(task.get("title"))
            if not task_title:
                continue
            tasks.append({
                "id": _take_id(book, task.get("id"), used),
                "title": task_title,
                "style": "task",
                "project": ident,
                "items": _clean_items(task.get("items")),
            })
        projects.append({
            "id": ident,
            "title": title,
            "style": "project",
            "items": _clean_items(raw.get("items")),
            "tasks": tasks,
        })
    for raw in loose:
        _place_loose_task(book, projects, raw, used)
    book["lists"] = projects
    return book


def load(path=None, seed_path=""):
    path = path or PATH
    raw = None
    try:
        with open(path, encoding="utf-8") as handle:
            raw = json.load(handle)
    except (OSError, ValueError):
        raw = None
    loose = _has_loose_tasks(raw)
    book = normalize(raw)
    seed = seed_path if isinstance(seed_path, str) else ""
    seeded = False
    if seed and not book["recent"] and os.path.isfile(seed) and is_drawing(seed):
        book["recent"].append({
            "path": os.path.abspath(seed),
            "opened": "",
            "done": False,
            "tag": user_name(),
        })
        seeded = True
    if loose or seeded:
        save(book, path)
    return book


def save(book, path=None):
    path = path or PATH
    os.makedirs(os.path.dirname(path), exist_ok=True)
    temp = path + ".tmp"
    with open(temp, "w", encoding="utf-8") as handle:
        json.dump(book, handle)
    os.replace(temp, path)


def find_list(book, list_id):
    for item in _walk_lists(book):
        if item.get("id") == list_id:
            return item
    return None


def lists_of(book, style, project_id=""):
    if style == "project":
        return [item for item in book.get("lists") or [] if item.get("style") == "project"]
    tasks = []
    for item in book.get("lists") or []:
        if item.get("style") != "project":
            continue
        if project_id and item.get("id") != project_id:
            continue
        tasks.extend(item.get("tasks") or [])
    return tasks


def parent_project(book, list_id):
    for item in book.get("lists") or []:
        if item.get("style") != "project":
            continue
        if item.get("id") == list_id:
            return item
        for task in item.get("tasks") or []:
            if task.get("id") == list_id:
                return item
    return None


def projects_for(book, path):
    path = os.path.abspath(path) if isinstance(path, str) else ""
    found = []
    if not path:
        return found
    for project in lists_of(book, "project"):
        paths = {item["path"] for item in project.get("items") or []}
        for task in project.get("tasks") or []:
            paths.update(item["path"] for item in task.get("items") or [])
        if path in paths:
            found.append(project)
    return found


def progress(lst):
    items = lst.get("items") or []
    done = sum(1 for item in items if item.get("done"))
    return done, len(items)


def make_list(book, title, style, project_id=""):
    title = _title(title)
    if not title:
        return None, "Type a title."
    if style not in STYLES:
        return None, "Choose a project or a task."
    if style == "task":
        project = find_list(book, project_id)
        if project is None or project.get("style") != "project":
            return None, "Pick a project."
        for item in project.get("tasks") or []:
            if item["title"].lower() == title.lower():
                return None, "That task list already has this title."
        book["seq"] = int(book.get("seq") or 0) + 1
        row = {
            "id": "l%d" % book["seq"],
            "title": title,
            "style": "task",
            "project": project["id"],
            "items": [],
        }
        project.setdefault("tasks", []).append(row)
        return row, ""
    for item in lists_of(book, "project"):
        if item["title"].lower() == title.lower():
            return None, "That project list already has this title."
    book["seq"] = int(book.get("seq") or 0) + 1
    row = {"id": "l%d" % book["seq"], "title": title, "style": "project", "items": [], "tasks": []}
    book["lists"].append(row)
    return row, ""


def rename_list(book, list_id, title):
    lst = find_list(book, list_id)
    if lst is None:
        return "That list is gone."
    title = " ".join(str(title or "").split())
    if not title:
        return "Type a title."
    if lst.get("style") == "task":
        project = parent_project(book, list_id)
        siblings = [] if project is None else project.get("tasks") or []
    else:
        siblings = lists_of(book, "project")
    for item in siblings:
        if item["id"] != list_id and item["title"].lower() == title.lower():
            return "That %s list already has this title." % lst["style"]
    lst["title"] = title
    return ""


def drop_list(book, list_id):
    removed = False
    kept = []
    for item in book.get("lists") or []:
        if item.get("id") == list_id:
            removed = True
            continue
        before = list(item.get("tasks") or [])
        item["tasks"] = [task for task in before if task.get("id") != list_id]
        if len(item["tasks"]) != len(before):
            removed = True
        kept.append(item)
    book["lists"] = kept
    return removed


def find_item(book, list_id, path):
    lst = find_list(book, list_id)
    if lst is None:
        return None
    path = os.path.abspath(path)
    for item in lst["items"]:
        if item["path"] == path:
            return item
    return None


def list_hits(book, path):
    path = os.path.abspath(path)
    hits = []
    for lst in _walk_lists(book):
        for item in lst.get("items") or []:
            if item["path"] == path:
                hits.append((lst, item))
    return hits


def add_paths(book, list_id, paths, tag=None):
    lst = find_list(book, list_id)
    added, duplicate, rejected = [], [], []
    if lst is None:
        return {"added": added, "duplicate": duplicate, "rejected": list(paths or [])}
    tag = " ".join(str(tag or "").split()) or user_name()
    have = {item["path"] for item in lst["items"]}
    for raw in paths or []:
        if not isinstance(raw, str) or not raw:
            continue
        path = os.path.abspath(raw)
        if not is_drawing(path) or not os.path.isfile(path):
            rejected.append(path)
            continue
        if path in have:
            duplicate.append(path)
            continue
        have.add(path)
        lst["items"].append({
            "path": path,
            "label": os.path.basename(path),
            "tag": tag,
            "done": False,
        })
        added.append(path)
    if added and lst.get("style") == "task":
        project = parent_project(book, lst["id"])
        if project is not None:
            have = {item["path"] for item in project.get("items") or []}
            for path in added:
                if path in have:
                    continue
                source = find_item(book, lst["id"], path)
                project.setdefault("items", []).append({
                    "path": path,
                    "label": source.get("label") if source else os.path.basename(path),
                    "tag": source.get("tag") if source else tag,
                    "done": bool(source.get("done")) if source else False,
                })
                have.add(path)
    return {"added": added, "duplicate": duplicate, "rejected": rejected}


def set_fields(book, list_id, path, label=None, tag=None, done=None):
    item = find_item(book, list_id, path)
    if item is None:
        return False
    if label is not None:
        text = " ".join(str(label).split())
        if not text:
            return False
        item["label"] = text
    if tag is not None:
        item["tag"] = " ".join(str(tag).split()) or user_name()
    if done is not None:
        item["done"] = bool(done)
    return True


def drop_item(book, list_id, path):
    lst = find_list(book, list_id)
    if lst is None:
        return False
    path = os.path.abspath(path)
    before = len(lst["items"])
    lst["items"] = [item for item in lst["items"] if item["path"] != path]
    return len(lst["items"]) != before


def recent_row(book, path):
    path = os.path.abspath(path)
    for item in book.get("recent") or []:
        if item["path"] == path:
            return item
    return None


def touch_recent(book, path, when=None):
    path = os.path.abspath(path)
    when = when or time.strftime("%Y-%m-%d %H:%M")
    kept = None
    rest = []
    for item in book.get("recent") or []:
        if item["path"] == path and kept is None:
            kept = item
        elif item["path"] != path:
            rest.append(item)
    if kept is None:
        kept = {"path": path, "opened": when, "done": False, "tag": user_name()}
    else:
        kept["opened"] = when
        kept.setdefault("done", False)
        if not kept.get("tag"):
            kept["tag"] = user_name()
    book["recent"] = [kept] + rest
    book["recent"] = book["recent"][:RECENT_LIMIT]


def set_done_for_path(book, path, done):
    path = os.path.abspath(path)
    found = False
    for _lst, item in list_hits(book, path):
        item["done"] = bool(done)
        found = True
    recent = recent_row(book, path)
    if recent is not None:
        recent["done"] = bool(done)
        found = True
    return found


def done_for(book, path, item=None):
    """Finished state for one row. A list row uses that row. Recent uses every list."""
    if item is not None:
        return bool(item.get("done"))
    hits = list_hits(book, path)
    if hits:
        return all(entry.get("done") for _lst, entry in hits)
    recent = recent_row(book, path)
    if recent is None:
        return None
    return bool(recent.get("done"))


def label_tag(book, path, item=None):
    if item is not None:
        return item.get("label") or os.path.basename(path), item.get("tag") or ""
    hits = list_hits(book, path)
    if hits:
        entry = hits[0][1]
        return entry.get("label") or os.path.basename(path), entry.get("tag") or ""
    recent = recent_row(book, path)
    tag = (recent or {}).get("tag") or ""
    return os.path.basename(path), tag


def state_text(done, exists):
    if not exists:
        return "missing"
    if done is True:
        return "finished"
    if done is False:
        return "working"
    return ""


def size_text(path):
    try:
        size = os.path.getsize(path)
    except OSError:
        return ""
    if size < 1024:
        return "%d B" % size
    if size < 1024 * 1024:
        return "%d KB" % (size // 1024)
    return "%d MB" % (size // (1024 * 1024))


def open_hint(path):
    ext = os.path.splitext(path or "")[1].lower()
    if ext == ".clip":
        return (
            "A .clip is converted into paint when you open it. "
            "A large file can take a minute, and opening it again converts it again. "
            "Save a blueprint from the studio when you want the fast open."
        )
    if ext == ".vmib":
        return "A blueprint opens the saved project and skips the Clip Studio conversion."
    if ext in (".psd", ".psb"):
        return "A Photoshop file is read directly. A large one still takes time to decode."
    if ext == ".kra":
        return "A Krita file is read directly. A large one still takes time to decode."
    if ext == ".xcf":
        return "A GIMP file is read directly. A large one still takes time to decode."
    return ""


def selection_note(path):
    if not path:
        return "Select a drawing. Nothing loads until you open it."
    if not os.path.isfile(path):
        return "That file is missing. It stays on the list until you remove it."
    size = size_text(path)
    hint = open_hint(path)
    if size and hint:
        return "%s. %s" % (size, hint)
    return hint or size or os.path.basename(path)


def same_drawing(current, path, loading=False):
    if loading or not current or not path:
        return False
    try:
        return os.path.abspath(current) == os.path.abspath(path)
    except (OSError, ValueError):
        return False


def places(home=None):
    home = home or os.path.expanduser("~")
    rows = [("Home", home)]
    for label, path in (
        ("Work", os.path.join(home, "Documents", "work")),
        ("Documents", os.path.join(home, "Documents")),
        ("Desktop", os.path.join(home, "Desktop")),
        ("Downloads", os.path.join(home, "Downloads")),
    ):
        if os.path.isdir(path) and os.path.abspath(path) != os.path.abspath(home):
            rows.append((label, path))
    rows.append(("Computer", os.path.abspath(os.sep)))
    return rows


def default_browse(book, home=None):
    home = home or os.path.expanduser("~")
    saved = (book or {}).get("browse") or ""
    if saved and os.path.isdir(saved):
        return os.path.abspath(saved)
    work = os.path.join(home, "Documents", "work")
    if os.path.isdir(work):
        return work
    return home


def _globs(text, exts):
    text = (text or "").strip()
    if not text:
        return ["*" + ext for ext in exts]
    lower = text.lower()
    if any(lower.endswith(ext) for ext in exts):
        if "*" not in text and "?" not in text:
            return ["*" + text]
        return [text]
    return ["*%s*%s" % (text, ext) for ext in exts]


def drawing_globs(text):
    return _globs(text, DRAWING_EXTS)


def image_globs(text):
    return _globs(text, IMAGE_EXTS)
