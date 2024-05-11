# A one-off tool for the migration when we added `Competitor` concept to
# distinguish between games played by registered users and games by guests with
# the same nickname.

import sqlite3


def get_registered_users():
    con = sqlite3.connect("bughouse-secret.db")
    cur = con.cursor()
    res = cur.execute("SELECT user_name FROM accounts").fetchall()
    con.close()
    return set([name for (name,) in res])


def fix_name(rowid: int, name: str, registered_users: set[str], rated: bool):
    assert "/" not in name, rowid
    registered = name in registered_users
    if rated:
        assert registered, rowid
    if registered:
        return f"user/{name}"
    else:
        return f"guest/{name}"


def fix_player_names(registered_users: set[str]):
    con = sqlite3.connect("bughouse.db")
    cur = con.cursor()
    cur.execute(
        "SELECT rowid, player_red_a, player_red_b, player_blue_a, player_blue_b, rated FROM finished_games"
    )
    rows = cur.fetchall()
    for row in rows:
        rowid, player_red_a, player_red_b, player_blue_a, player_blue_b, rated = row
        fixed_player_red_a = fix_name(rowid, player_red_a, registered_users, rated)
        fixed_player_red_b = fix_name(rowid, player_red_b, registered_users, rated)
        fixed_player_blue_a = fix_name(rowid, player_blue_a, registered_users, rated)
        fixed_player_blue_b = fix_name(rowid, player_blue_b, registered_users, rated)
        cur.execute(
            """
                UPDATE finished_games
                SET player_red_a = ?, player_red_b = ?, player_blue_a = ?, player_blue_b = ?
                WHERE rowid = ?
            """,
            (
                fixed_player_red_a,
                fixed_player_red_b,
                fixed_player_blue_a,
                fixed_player_blue_b,
                rowid,
            ),
        )
    con.commit()
    con.close()


registered_users = get_registered_users()
print("Registered_users: ", sorted(registered_users))
fix_player_names(registered_users)
