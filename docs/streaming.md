# Streaming the game's files

Keep the game's 20 GB on one Mac and play it on your other Macs on the
same network: the files come over the network as the game asks for them,
and stay on the Mac for the next time. The game itself always runs on the
Mac you play on; only its files travel.

## Share them from a Mac

On the Mac with the game's files, either:

- open re5OSX and turn on **Settings › Game › Share on the network**; it
  shares while the app is open, or
- run `re5osx-serve`, the same server without the app (`mise run serve
  [folder]` from the source; `-- --help` lists its options).

It listens on port 47100 and announces itself on the local network.

## Play from it

On the other Mac: **Stream from a server** on the start page, or
**Settings › Game › Game files › Server**. The **Servers** page lists the
Macs on the network that share the game, whether each has the right build
and how far it is. For a computer elsewhere, type its address (`host` or
`host:port`).

The first start downloads what the game needs to start (about a minute
on a home network); the rest comes while you play, in the background. On
the same page, **Download the whole game** copies everything, so the Mac
plays without the server.

## How it works

The game's engine already reads its files as it would from the web, and
announces the reads it is about to make. re5OSX answers each read from
the Mac's copy when it is there, as fast as a local folder; a missing
part is asked of the server, and the announcements become requests ahead
of time. One TCP connection carries them, the parts the game is waiting
for first, each checked on arrival. The copy grows as you play and serves
the next sessions. The details are in the
[design notes](design.md#streaming-the-games-files).
