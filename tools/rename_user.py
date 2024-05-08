# WARNING: This is a primitive tool that is not guaranteed to correctly replace
# all possible user names! It basically does a text replacements with a few
# sanity checks. So if somebody changes they name from "e4" to "e5", many PGNs
# will become corrupted.

import re
import sqlite3

OLD_NAME = "old_name"
NEW_NAME = "new_name"
OLD_NAME_RE = re.compile(r"\b" + re.escape(OLD_NAME) + r"\b")


def replace_name(text):
    return re.sub(OLD_NAME_RE, NEW_NAME, text)


def fix_secret_db():
    con = sqlite3.connect("bughouse-secret.db")
    cur = con.cursor()
    cur.execute("SELECT rowid, user_name FROM accounts")
    matching_rowid = None
    for row in cur.fetchall():
        rowid, user_name = row
        if re.match(OLD_NAME_RE, user_name):
            assert user_name == OLD_NAME
            assert matching_rowid is None
            matching_rowid = rowid
    assert matching_rowid is not None
    cur.execute(
        "UPDATE accounts SET user_name = ? WHERE rowid = ?", (NEW_NAME, matching_rowid)
    )
    con.commit()
    con.close()


def fix_main_db():
    con = sqlite3.connect("bughouse.db")
    cur = con.cursor()
    cur.execute(
        "SELECT rowid, player_red_a, player_red_b, player_blue_a, player_blue_b, game_pgn FROM finished_games"
    )
    rows = cur.fetchall()
    for row in rows:
        rowid, player_red_a, player_red_b, player_blue_a, player_blue_b, game_pgn = row
        fixed_player_red_a = replace_name(player_red_a)
        fixed_player_red_b = replace_name(player_red_b)
        fixed_player_blue_a = replace_name(player_blue_a)
        fixed_player_blue_b = replace_name(player_blue_b)
        fixed_game_pgn = replace_name(game_pgn)
        cur.execute(
            """
                UPDATE finished_games
                SET player_red_a = ?, player_red_b = ?, player_blue_a = ?, player_blue_b = ?, game_pgn = ?
                WHERE rowid = ?
            """,
            (
                fixed_player_red_a,
                fixed_player_red_b,
                fixed_player_blue_a,
                fixed_player_blue_b,
                fixed_game_pgn,
                rowid,
            ),
        )
    con.commit()
    con.close()


fix_secret_db()
fix_main_db()
