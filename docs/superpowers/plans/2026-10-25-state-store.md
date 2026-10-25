# Estado durável do serviço — plano de implementação

Data: 2026-10-25. Escopo: intenção e registro de torrents; integração com o motor vem depois.

## Contrato

- Um único dono abre o banco em um diretório de estado escolhido explicitamente. O handle de lock acompanha a conexão por toda a vida do serviço.
- `register` recebe hash v1 validado, ID estável, metadados/origem limitados e intenção inicial. O mesmo hash devolve o registro existente se os parâmetros forem idênticos; dados divergentes produzem conflito.
- `set_intent` troca `running`/`paused` em transação; operação repetida é idempotente. A intenção é distinta do estado observado do motor.
- `remove` grava tombstone em transação e nunca apaga o conteúdo. Um novo `register` do mesmo hash tombstoned produz conflito até uma política explícita de novo cadastro ser definida.
- Um esquema desconhecido, mais recente ou inconsistente falha fechado. Não recriar banco ou dados automaticamente.
- A lista de recuperação lê só registros ativos e sua intenção. O motor sempre revalida peças ao iniciar; checkpoints não são autoridade.

## TDD e entrega

1. Testes com arquivo SQLite temporário para cadastro idempotente, conflito de origem/destino, limite de 100 registros e tamanho de metadados.
2. Testes de persistência após fechar/reabrir para pausa, retomada e tombstone; `recover` não inclui removidos. Testar versão futura sem alterar o arquivo.
3. Implementar esquema v1 e operações transacionais por uma conexão dona do serviço. SQL parametrizado e `PRAGMA foreign_keys=ON`; configurar durabilidade, lock exclusivo de processo e tempo limite de banco explicitamente.
4. Rodar fmt, clippy e testes nos três SOs; documentar o que ainda falta para recuperação de processo completo, lock, backup de migração e integração de identidades de arquivo.

Este milestone não habilita downloads pela CLI até que armazenamento, destinos de rede e a reconciliação com o motor estejam qualificados.
