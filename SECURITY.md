# Security policy

There is no supported production release yet. The optional experimental service can download v1 torrents and tracker-backed magnets. Downloaded content is untrusted: torrent hashes check integrity against metainfo, not whether files are safe to open or run.

Do not put credentials, private torrent information or exploitable vulnerability details in public issues. Private vulnerability reporting is enabled: use [Report a vulnerability](https://github.com/MiguelNevesCamargoJunior/torrent-lite/security/advisories/new). No response-time SLA is offered during early development.

The experimental service uses authenticated loopback control, bounded metainfo input, capability-scoped file access and an outbound policy proxy. The browser dashboard uses a separate, temporary read-only URL. DHT, UDP trackers and web seeds remain disabled while their network boundary is unresolved. There has been no independent security audit or stable release. Report possible escapes from the download root, private-address connections, cross-origin control, or credential leaks privately through the link above.

