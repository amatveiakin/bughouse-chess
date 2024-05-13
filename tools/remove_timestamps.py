# A one-off tool for removing broken timestamps generated between
# 234ed2b1b5e46028e8732c537e8d42ac1c419f10 and
# fbce1728c28335f5618de2d7845fda6d4499db27.

import os
import sqlite3
import subprocess

MIN_ROWID = 2773
MAX_ROWID = 2828  # inclusive
BUGHOUSE_ROOT = os.environ["BUGHOUSE_ROOT"]
BUGHOUSE_BINARY = f"{BUGHOUSE_ROOT}/target/release/bughouse_console"


con = sqlite3.connect("bughouse.db")
cur = con.cursor()
cur.execute(
    "SELECT rowid, game_pgn FROM finished_games WHERE rowid BETWEEN ? AND ?",
    (MIN_ROWID, MAX_ROWID),
)
rows = cur.fetchall()
updated = 0
not_updated = 0
failed = 0
for row in rows:
    rowid, game_pgn = row
    ret = subprocess.run(
        [BUGHOUSE_BINARY, "bpgn", "--remove-timestamps"],
        input=game_pgn,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        # stderr=subprocess.DEVNULL,
    )
    if ret.returncode == 0:
        if ret.stdout == game_pgn:
            not_updated += 1
        else:
            cur.execute(
                "UPDATE finished_games SET game_pgn = ? WHERE rowid = ?",
                (ret.stdout, rowid),
            )
            updated += 1
    else:
        failed += 1
con.commit()
con.close()

print(f"Updated:     {updated}")
print(f"Not updated: {not_updated}")
print(f"Failed:      {failed}")
