# Fila de downloads (primeira versão)

O serviço aceita `--max-active N` entre 1 e 100, com padrão 3. No máximo N torrents incompletos com intenção `running` podem ficar despausados ao mesmo tempo. Itens excedentes ficam em fila, em ordem de cadastro, sem conexão de rede até sua vez. O identificador e a posição sobrevivem a reinícios no mesmo banco SQLite.

Um item `paused` nunca ocupa vaga. Ao pausar, remover ou concluir um download, o próximo item elegível entra em execução. Uma tarefa do comando `serve` reconcilia a fila a cada dois segundos para detectar conclusão mesmo sem clientes conectados. As mutações usam a mesma trava para que comandos simultâneos não ultrapassem o limite. A verificação de arquivos respeita o limite ao terminar.

Magnets sem metainfo reservam uma vaga enquanto a busca ocorre em segundo plano. A busca não segura a trava de mutações; pausa, remoção e parada cancelam sua tarefa. O resultado é validado e salvo antes da ativação do download. Falhas entram em recuo limitado, liberando a vaga para outros itens.

A API relata `observed_state: "resolving"` enquanto busca metainfo de um magnet e `"queued"` para itens `running` sem handle nem busca ativa. O painel mostra “Resolvendo metadados” ou “Na fila”. Torrents concluídos permanecem visíveis e são pausados automaticamente; política de seeding configurável fica para outra etapa.

Testes necessários: ordem persistida, limite durante cadastro e retomada, preenchimento de vaga após pausa e conclusão, e reinício com itens excedentes. Antes de release estável, medir throughput e memória com diferentes limites e testar falhas de ativação de trackers/metadados.
