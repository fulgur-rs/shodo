"""Portable path names for saved evidence; live execution paths stay absolute."""
import json
import re
from pathlib import Path


def portable(value, root):
    """Use ./ for this checkout and ~/ for the current home, including diagnostics.

    Preserve other absolute paths (such as /usr/bin/perf), numbers and digests.
    Match complete directory prefixes so a similarly named sibling is untouched.
    """
    prefixes = [(str(Path(root).resolve()), "."), (str(Path.home()), "~")]
    patterns = [(re.compile(r"(?:(?<=file://)|(?<=[\s\"'=\[\]({:,;]))" + re.escape(path)
                            + r"(?=/|$|[\"'#\]})]|\.(?=$|\s)|[:,;](?=$|\s))"), name)
                for path, name in prefixes if path != "/"]

    def convert(item):
        if isinstance(item, str):
            # A path value may contain spaces and punctuation in directory
            # names. Only an actual slash or the end terminates its prefix.
            if item.startswith("/"):
                for path, name in prefixes:
                    if path != "/" and (item == path or item.startswith(path + "/")):
                        item = name + item[len(path):]
                        break
            # Diagnostics and Cargo file URIs can embed a path in other text.
            # Do not mistake a suffix inside another absolute path for home.
            for pattern, name in patterns:
                item = pattern.sub(name, item)
            return item
        if isinstance(item, dict):
            return {convert(key): convert(value) for key, value in item.items()}
        if isinstance(item, (list, tuple)):
            return [convert(value) for value in item]
        return item

    return convert(value)


def write_json(path, value, root):
    Path(path).write_text(json.dumps(portable(value, root), indent=2,
                                    ensure_ascii=False, allow_nan=False) + "\n")
