"""Packs Clauseo's Python runtime into a deterministic .tar.gz.

Run by the runtime's own Python from `cargo xtask python-runtime build`:

    python -I -B pack.py <runtime dir> <top folder name> <archive path>

The same tree packs to the same bytes on the same runtime: entries in sorted order, every
timestamp fixed, owner 0:0 with no names, modes 0755 for folders and executables and 0644 for
other files, PAX format, and a gzip header with no name and mtime 0. Symlinks, hard links and any
other special file fail the pack: Clauseo's extractor would skip them.
"""

import gzip
import os
import stat
import sys
import tarfile

# 2024-01-01T00:00:00Z, the time PBS stamps its own archives with.
MTIME = 1704067200


def entries(root):
    """Every path under root, relative with '/', folders before what is in them, sorted."""
    found = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        rel = os.path.relpath(dirpath, root)
        for name in dirnames + sorted(filenames):
            found.append(name if rel == "." else os.path.join(rel, name).replace(os.sep, "/"))
    return sorted(found)


def info(root, rel, top):
    path = os.path.join(root, rel)
    st = os.lstat(path)
    item = tarfile.TarInfo(f"{top}/{rel}")
    item.mtime = MTIME
    item.uid = item.gid = 0
    item.uname = item.gname = ""
    if stat.S_ISDIR(st.st_mode):
        item.type = tarfile.DIRTYPE
        item.mode = 0o755
    elif stat.S_ISREG(st.st_mode):
        item.type = tarfile.REGTYPE
        item.size = st.st_size
        executable = os.name != "nt" and st.st_mode & 0o111
        item.mode = 0o755 if executable else 0o644
    else:
        raise SystemExit(f"pack.py: {rel} is not a file or a folder; the extractor would skip it")
    return item, path


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: pack.py <runtime dir> <top folder name> <archive path>")
    root, top, archive = sys.argv[1], sys.argv[2], sys.argv[3]
    with open(archive, "xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as gz:
            with tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as tar:
                top_info = tarfile.TarInfo(top)
                top_info.type, top_info.mode, top_info.mtime = tarfile.DIRTYPE, 0o755, MTIME
                top_info.uname = top_info.gname = ""
                tar.addfile(top_info)
                for rel in entries(root):
                    item, path = info(root, rel, top)
                    if item.type == tarfile.REGTYPE:
                        with open(path, "rb") as body:
                            tar.addfile(item, body)
                    else:
                        tar.addfile(item)


if __name__ == "__main__":
    main()
