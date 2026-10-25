# Network destination boundary

Date: 2026-10-25. This is a gate for enabling real downloads, not a claim that the engine is already confined.

The default mode permits outbound connections only to public unicast IP addresses. The decision is made on the concrete IP used by the socket, after DNS resolution. DNS answers and redirects are rechecked for every new connection. Numeric peers follow the same rule. A local authenticated SOCKS5 proxy is the proposed enforcement point for the engine's TCP peer and HTTP(S) tracker traffic. Its listener must be loopback-only, require random credentials, cap concurrent connections, and pin an approved resolved IP when connecting. No direct engine TCP fallback is allowed.

The current engine's SOCKS option covers outbound TCP peers and its reqwest HTTP client, but its UDP tracker client resolves and sends directly. Until an audited path exists, all UDP trackers and DHT, uTP and local discovery must be disabled for production sessions. Magnet and metainfo trackers must be sanitized before reaching the engine; rejecting them after a connection is too late. HTTP(S) redirects must continue through the proxy. Torrent URLs and web seeds are not accepted as download input.

First tests: public IPv4/IPv6 accepted; loopback, private, link-local, documentation, multicast, IPv4-mapped IPv6, 6to4 and unspecified addresses denied. Tracker URLs accept only HTTP(S) without credentials or fragments. This pure policy module precedes the proxy so the dangerous address classes are explicit and independently testable. It does not on its own enforce a network boundary.

Source of address classes: IANA IPv4 and IPv6 Special-Purpose Address Registries, consulted 2026-10-25. Update the policy when those registries change.

- https://www.iana.org/assignments/iana-ipv4-special-registry/
- https://www.iana.org/assignments/iana-ipv6-special-registry/
