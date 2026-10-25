# Seleção de arquivos

## Contrato

- A lista de arquivos segue a ordem de índice do metainfo v1 e da inspeção `inspect-torrent`. O índice começa em zero. A UI apresenta caminho, tamanho e índice; texto do torrent é tratado apenas como texto.
- A seleção omitida significa todos os arquivos. Uma seleção explícita é uma lista não vazia de índices únicos em ordem crescente, limitada a 10.000 entradas e validada contra o metainfo. Índice inválido ou repetido é recusado sem modificar o estado.
- A escolha é gravada antes da ativação do motor e sobrevive a reinício e verificação. Registros antigos migram com seleção omitida. A alteração de seleção exige torrent pausado e metainfo disponível; retomar aplica a seleção persistida.
- Para magnets ainda sem metainfo, a seleção só pode ser feita após a resolução. Nenhum nome de arquivo vindo de magnet ou `.torrent` entra em logs, HTML ou caminho sem a validação existente.
- O motor solicita peças que intersectam arquivos escolhidos. Quando uma peça cruza a fronteira, bytes do arquivo não escolhido podem ser necessários para validar a peça e ser gravados. Arquivos não escolhidos sem peças compartilhadas não são criados nem pré-alocados.
- Progresso e conclusão representam as peças selecionadas, com a semântica que o motor realmente reporta. A UI mostra claramente o tamanho selecionado e o total do torrent.

## Testes de aceitação

1. Fixture multifile de três arquivos e peças alinhadas: escolher apenas o índice 1 baixa bytes iguais à origem, não cria os demais e completa.
2. Seleção inválida ou duplicada não altera o registro; seleção legítima persiste após reinício e `verify`.
3. Alterar seleção enquanto pausado, retomar e concluir baixa o novo conjunto; alteração durante execução é recusada.
4. Magnet resolvido aceita seleção; magnet pendente retorna estado de metadados indisponíveis.
5. Peça na fronteira de arquivos continua verificável sem prometer ausência física absoluta de bytes não escolhidos.
