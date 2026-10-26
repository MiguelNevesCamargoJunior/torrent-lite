# Falha de escrita por falta de espaço

Um teste de integração com seeder local usa um adaptador de armazenamento que aceita somente um orçamento pequeno de bytes. Quando a escrita excede o orçamento, ele grava o prefixo permitido e retorna `StorageFull`. O cliente não pode declarar o torrent completo. O teste encerra essa sessão, mantém banco e arquivo parcial, inicia outra sessão com o armazenamento normal e exige rechecagem por hash antes de completar o download. O conteúdo final deve ser idêntico ao da origem e a identidade do arquivo deve continuar persistida.

O erro é injetado pelo teste na interface de armazenamento do motor; o binário distribuído não recebe um modo de falha. A matriz hospedada executa o cenário em Windows, Linux e macOS. Isto não substitui testes de cota real, alocação em disco cheio, arquivos bloqueados ou queda de energia.
