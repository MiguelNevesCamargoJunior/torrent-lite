# Security policy

There is no supported production release yet. The current CLI only inspects magnet input offline. It neither downloads content nor guarantees content safety.

Do not put credentials, private torrent information or exploitable vulnerability details in public issues. Private vulnerability reporting is enabled: use [Report a vulnerability](https://github.com/MiguelNevesCamargoJunior/torrent-lite/security/advisories/new). No response-time SLA is offered during early development.

The design requires authenticated loopback control, bounded untrusted input, storage confinement including Windows reparse races, and outbound destination filtering. Those protections are not implied by the lexical path validator. The proposed engine remains unqualified for production integration until these gates pass.

