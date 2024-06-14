import os
import sqlite3
import subprocess

BUGHOUSE_ROOT = os.environ["BUGHOUSE_ROOT"]
BUGHOUSE_BINARY = f"{BUGHOUSE_ROOT}/target/release/bughouse_console"
PURGE_GAMES = False


def check_user_name(user_name: str) -> bool:
    ret = subprocess.run(
        [BUGHOUSE_BINARY, "check-user-name", user_name],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return ret.returncode == 0


def get_registered_users() -> set[str]:
    con = sqlite3.connect("bughouse-secret.db")
    cur = con.cursor()
    res = cur.execute("SELECT user_name FROM accounts").fetchall()
    con.close()
    return set([name for (name,) in res])


def get_guest_players() -> set[str]:
    GUEST_PREFIX = "guest/"
    con = sqlite3.connect("bughouse.db")
    cur = con.cursor()
    cur.execute(
        "SELECT player_red_a, player_red_b, player_blue_a, player_blue_b FROM finished_games"
    )
    rows = cur.fetchall()
    guest_players = set()
    for row in rows:
        player_red_a, player_red_b, player_blue_a, player_blue_b = row
        for p in [player_red_a, player_red_b, player_blue_a, player_blue_b]:
            if p.startswith(GUEST_PREFIX):
                p = p[len(GUEST_PREFIX) :]
                guest_players.add(p)
    con.close()
    return guest_players


def get_bad_rows(registered_users: set[str], bad_names: set[str]) -> list[int]:
    GUEST_PREFIX = "guest/"
    USER_PREFIX = "user/"
    con = sqlite3.connect("bughouse.db")
    cur = con.cursor()
    cur.execute(
        "SELECT rowid, player_red_a, player_red_b, player_blue_a, player_blue_b FROM finished_games"
    )
    rows = cur.fetchall()
    bad_rows = []
    for row in rows:
        bad_row = False
        rowid, player_red_a, player_red_b, player_blue_a, player_blue_b = row
        for p in [player_red_a, player_red_b, player_blue_a, player_blue_b]:
            if p.startswith(GUEST_PREFIX):
                p = p[len(GUEST_PREFIX) :]
            elif p.startswith(USER_PREFIX):
                p = p[len(USER_PREFIX) :]
                assert p in registered_users, p
            else:
                assert False, f"Invalid competitor id: {p}"
            if p in bad_names:
                bad_row = True
        if bad_row:
            bad_rows.append(rowid)
    con.close()
    return bad_rows


def delete_rows(bad_rows: list[int]):
    con = sqlite3.connect("bughouse.db")
    cur = con.cursor()
    for rowid in bad_rows:
        cur.execute("DELETE FROM finished_games WHERE rowid = ?", (rowid,))
    con.commit()
    con.close()


def check_names_set(caption: str, names: set[str], all_bad_names: set[str]):
    total = 0
    bad_names = []
    for name in names:
        total += 1
        if not check_user_name(name):
            bad_names.append(name)
    all_bad_names |= set(bad_names)
    print(f"{caption}: {len(bad_names)} of {total} names invalid: {bad_names}")


bad_names = set()
registered_users = get_registered_users()
guest_players = get_guest_players()
check_names_set("Registered users", registered_users, bad_names)
check_names_set("Guest players", guest_players, bad_names)
bad_rows = get_bad_rows(registered_users, bad_names)
print(f"Bad rows: {bad_rows}")
if PURGE_GAMES:
    delete_rows(bad_rows)
    print("Rows deleted")
