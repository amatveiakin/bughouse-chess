import functools
import os
import sqlite3
import subprocess

BUGHOUSE_ROOT = os.environ["BUGHOUSE_ROOT"]
BUGHOUSE_BINARY = f"{BUGHOUSE_ROOT}/target/release/bughouse_console"


def append_to_ranges(ranges: list[tuple[int, int]], x: int) -> list[tuple[int, int]]:
    if len(ranges) == 0:
        return [(x, x)]
    else:
        (start, end) = ranges[-1]
        if x == end + 1:
            ranges[-1] = (start, x)
        else:
            ranges.append((x, x))
        return ranges


def group_to_str(group: tuple[int, int]) -> str:
    start, end = group
    if start == end:
        return f"{start}"
    else:
        return f"{start}-{end}"


def format_rowid_list(items: list[int]) -> str:
    MAX_GROUPS = 10
    groups = functools.reduce(append_to_ranges, items, [])
    groups_str = (
        ", ".join(group_to_str(g) for g in groups)
        if len(groups) <= MAX_GROUPS
        else "..., " + ", ".join(group_to_str(g) for g in groups[(-MAX_GROUPS + 1) :])
    )
    num_items = len(items)
    return f"{num_items:6}  ({groups_str})"


con = sqlite3.connect("bughouse.db")
cur = con.cursor()
cur.execute("SELECT rowid, game_pgn FROM finished_games")
rows = cur.fetchall()
ok_stable = []
ok_changed = []
failed = []
for row in rows:
    rowid, game_pgn = row
    ret = subprocess.run(
        # [BUGHOUSE_BINARY, "bpgn", "--role=client"],
        [BUGHOUSE_BINARY, "bpgn"],
        input=game_pgn,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    if ret.returncode == 0:
        if ret.stdout == game_pgn:
            ok_stable.append(rowid)
        else:
            ok_changed.append(rowid)
    else:
        failed.append(rowid)
con.close()

print(f"OK, stable:  {format_rowid_list(ok_stable)}")
print(f"OK, changed: {format_rowid_list(ok_changed)}")
print(f"Failed:      {format_rowid_list(failed)}")
