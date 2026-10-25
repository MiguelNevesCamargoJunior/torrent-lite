# Torrents privados com tracker HTTP(S)

Um metainfo v1 marcado `private=1` é aceito quando contém exatamente um tracker HTTP(S) permitido pela política de destinos. O `info_hash` e o bit privado do dicionário `info` permanecem iguais após a sanitização dos trackers. Sem tracker permitido, ou com mais de um tracker, o serviço recusa a entrada de forma explícita: o motor fixado usa apenas o primeiro tracker privado e ainda não foi qualificado para failover privado.

A sessão do serviço continua sem DHT e descoberta local. O motor não anuncia nem recebe PEX para metainfo privado. Trackers HTTP(S) usam o proxy de saída com validação de IP após DNS. Peers anunciados pelo tracker usam o mesmo proxy. A interface de inspeção mostra o bit `private`; arquivos continuam sujeitos às mesmas regras de caminho, identidade e verificação de peças.

## Testes de aceitação

1. Sanitização preserva `info_hash` e `private` para um tracker HTTP(S) válido; recusa zero ou múltiplos trackers privados.
2. Um swarm local privado transfere bytes idênticos, pausa, reinicia, revalida, detecta uma peça corrompida e repara a peça.
3. Tracker ou peer apontando a um IP proibido continua bloqueado pela política de saída.
4. Qualificação futura: failover entre múltiplos trackers privados, compatibilidade com trackers UDP e teste de ausência de PEX com peer instrumentado.
