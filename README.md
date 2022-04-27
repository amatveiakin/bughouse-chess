# Bughouse chess client/server

Application that works both as a client and as a server for
[bughouse chess](https://en.wikipedia.org/wiki/Bughouse_chess) — the
best<sup>[citation not needed]</sup> kind of chess.

It exists because alternative bughouse implementations use a fixed set of rules
while the rules in fact vary.

Supported rule variations:

- Starting position: classic or Fischer random (a.k.a. Chess960).
- Limits on where pawns can be dropped.
- Limits on drop aggression, e.g. cannot drop piece if that leads to checkmate.


## Usage

Running as a server:

```
$ bughouse-chess server
```

Running as a client:

```
$ bughouse-chess client <server-ip> <player-name> <team>
```

Note. Client requires a modern terminal with raw mode support.
Windows 10 cmd and most Linux terminal should work, while mingw, git bash,
cmd and powershell in older Windows versions may not.
