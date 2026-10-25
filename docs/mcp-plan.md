# Adaptador MCP stdio

Estado: implementação inicial em validação, 2026-10-25. O adaptador é um comando `torrent-lite mcp --state <state.db>` que descobre o serviço local já iniciado e usa o mesmo `LocalClient` da CLI. A saída padrão é exclusiva do transporte MCP; erros e diagnóstico usam stderr sem tokens, magnets ou URLs de tracker. Encerrar o processo MCP não encerra downloads.

Usar o SDK Rust oficial `rmcp` fixado em `3.6.0` ou versão posterior após revisão da árvore de dependências. Essa versão corrige a vulnerabilidade de cache sem limite divulgada em 2026-10-20 para o transporte HTTP; este projeto usará somente stdio, mas não deve fixar uma versão conhecida como vulnerável. O SDK suporta a revisão estável MCP `2026-07-28` e clientes da era de handshake `2025-11-25`. Preferir os recursos de negociação e serialização do SDK em vez de escrever JSON-RPC manualmente.

Ferramentas iniciais: `torrent_list`, `torrent_status`, `torrent_add_magnet`, `torrent_pause`, `torrent_resume`, `torrent_verify`, `torrent_remove`. A rechecagem explícita aceita registros `.torrent` e magnets com metainfo já resolvida e persistida. `--read-only` omite mutações de `tools/list` e recusa chamadas de alteração. IDs são UUIDs da aplicação. A resposta não expõe token, caminhos locais desnecessários nem URLs de tracker.

Testes: cliente SDK via stdio realiza descoberta/initialize conforme a revisão negociada, lista schemas determinísticos, consulta um serviço sintético, executa pausa e remoção autorizadas, verifica `--read-only` e confirma que stdout contém apenas mensagens MCP válidas. Cliente com serviço ausente recebe erro de ferramenta, sem criar sessão de download própria.

Fontes: [revisão MCP 2026-07-28](https://blog.modelcontextprotocol.io/posts/2026-07-28/), [SDK Rust oficial](https://github.com/modelcontextprotocol/rust-sdk), [advisory de segurança do SDK](https://github.com/modelcontextprotocol/rust-sdk/security/advisories/GHSA-4432-vg92-fqh6).
