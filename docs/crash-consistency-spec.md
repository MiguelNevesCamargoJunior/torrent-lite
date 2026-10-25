# Consistência do estado após interrupção

## Contrato desta etapa

As mutações transacionais de registro, intenção de execução, remoção e identidade de arquivo devem ser atômicas. Se o processo termina depois do SQL e antes de `COMMIT`, a próxima abertura do banco vê somente o estado anterior. O lock de processo deve ser liberado pelo sistema operacional. A execução normal após `COMMIT` permanece coberta pelos testes de estado existentes.

Uma injeção compilada somente em `cfg(test)` termina um subprocesso com código 86 exatamente antes de cada commit. O processo pai reabre o mesmo banco e verifica registros, intenção, tombstone e identidade. Esta injeção não existe no binário de serviço ou nos pacotes de prévia.

Um quinto caso interrompe a migração v3→v4 após `VACUUM INTO` e após as instruções SQL, antes do commit. Banco e backup devem conservar a versão v3 e o registro original. A abertura seguinte deve recusar sobrescrever o backup, com `MigrationBackupExists`; a recuperação manual exige inspecionar ambos os arquivos antes de renomear ou remover qualquer backup.

Estes testes qualificam cinco limites transacionais do `StateStore`; ainda faltam interrupções durante mutações do serviço, gravação de peças, seleção de arquivos, cache de metainfo e outras fases da migração. O roadmap permanece aberto até cobrir essas fases e falhas de disco/arquivos bloqueados em Windows nativo.
