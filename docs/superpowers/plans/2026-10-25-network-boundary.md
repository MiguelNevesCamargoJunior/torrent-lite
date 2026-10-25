# Network destination boundary

Date: 2026-10-25. This is a gate for enabling real downloads, not a claim that the engine is already confined.

The default mode permits outbound connections only to public unicast IP addresses. The decision is made on the concrete IP used by the socket, after DNS resolution. DNS answers and redirects are rechecked for every new connection. Numeric peers follow the same rule. A local authenticated SOCKS5 proxy is the proposed enforcement point for the engine's TCP peer and HTTP(S) tracker traffic. Its listener must be loopback-only, require random credentials, cap concurrent connections, and pin an approved resolved IP when connecting. No direct engine TCP fallback is allowed.

The current engine's SOCKS option covers outbound TCP peers and its reqwest HTTP client, but its UDP tracker client resolves and sends directly. Until an audited path exists, all UDP trackers and DHT, uTP and local discovery must be disabled for production sessions. Magnet and metainfo trackers must be sanitized before reaching the engine; rejecting them after a connection is too late. HTTP(S) redirects must continue through the proxy. Torrent URLs and web seeds are not accepted as download input.

First tests: public IPv4/IPv6 accepted; loopback, private, link-local, documentation, multicast, IPv4-mapped IPv6, 6to4 and unspecified addresses denied. Tracker URLs accept only HTTP(S) without credentials or fragments. This pure policy module precedes the proxy so the dangerous address classes are explicit and independently testable. It does not on its own enforce a network boundary.

The experimental proxy now implements SOCKS5 CONNECT with a 256-bit random password, loopback-only binding, bounded handshakes and 150 concurrent sessions. It resolves domain requests itself, filters each concrete IP, then connects to that IP so DNS cannot change the selected socket destination. Tests cover authentication, IP and DNS denial, and an explicit LAN-mode relay. A combined local-peer qualification test is being added for the engine and persisted storage; public trackers remain disabled in that test. Do not turn on unrestricted engine discovery based on the proxy test alone.

Before public trackers are enabled, sanitize the magnet and metainfo supplied to the engine. Retain only validated HTTP(S) trackers and return the count of dropped trackers for a visible warning. A `.torrent` rewrite must preserve the raw `info` bytes and therefore the v1 infohash. Reject private torrents until their tracker isolation is proven. Keep metadata size and tracker count bounded. Validate the output again and assert the original hash remains unchanged. This is a preprocessing gate; the proxy still checks each concrete network destination and each redirect.

Next qualification: a synthetic local HTTP tracker returns a loopback seeder. The leecher gets no initial peer list. The test succeeds only if the tracker announces and the peer data connection both traverse the authenticated proxy, while the persisted storage adapter receives the bytes. The LAN mode is explicit for this test; the default public policy still denies its loopback endpoints.

Source of address classes: IANA IPv4 and IPv6 Special-Purpose Address Registries, consulted 2026-10-25. Update the policy when those registries change.

- https://www.iana.org/assignments/iana-ipv4-special-registry/
- https://www.iana.org/assignments/iana-ipv6-special-registry/
