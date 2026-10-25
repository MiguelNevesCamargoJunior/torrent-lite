# Persisted engine storage qualification

Date: 2026-10-25. This adapter is not yet enabled by the user-facing CLI.

The engine factory receives one validated torrent ID, an already opened download root, and the single owning `StateStore`. Before touching files it checks every path and each stored index/path/length against the engine metadata. For a new file it uses exclusive creation through `SafeRoot`, records the handle's identity in SQLite, then gives the handle to the engine. For a previously recorded file it reopens through `SafeRoot` and requires the same file identity. Reads, writes and length changes must stay within the declared file size. Engine deletion hooks preserve content.

The synthetic loopback test transfers data using this adapter, closes the state owner, corrupts one piece, reopens the database and adapter, and requires a full recheck and repair. A separate replacement check confirms that a newly created file cannot satisfy an old identity. `fastresume` is disabled and public discovery is disabled in the test.

Expected limitation: a crash between exclusive file creation and the SQLite identity commit leaves an unclaimed file. The next run fails closed instead of overwriting it. A future recovery flow must verify such a file against piece hashes before adopting it; no automatic adoption is enabled here. Network destination filtering, private torrents, resource caps and controlled crash tests remain necessary before exposing a downloader.
