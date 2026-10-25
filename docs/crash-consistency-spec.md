# Consistência do estado após interrupção

## Contrato desta etapa

As mutações transacionais de registro, intenção de execução, remoção e identidade de arquivo devem ser atômicas. Se o processo termina depois do SQL e antes de `COMMIT`, a próxima abertura do banco vê somente o estado anterior. O lock de processo deve ser liberado pelo sistema operacional. A execução normal após `COMMIT` permanece coberta pelos testes de estado existentes.

Uma injeção compilada somente em `cfg(test)` termina um subprocesso com código 86 exatamente antes de cada commit. O processo pai reabre o mesmo banco e verifica registros, intenção, tombstone e identidade. Esta injeção não existe no binário de serviço ou nos pacotes de prévia.

Este teste qualifica os quatro limites transacionais do `StateStore`; ainda faltam interrupções durante mutações do serviço, gravação de peças, seleção de arquivos, cache de metainfo e migração. O roadmap permanece aberto até cobrir essas fases e falhas de disco/arquivos bloqueados em Windows nativo.
