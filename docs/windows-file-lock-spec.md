# Arquivo bloqueado no Windows

Um teste nativo do Windows registra um `.torrent` pausado e a identidade do arquivo de destino. Outra abertura mantém o arquivo com compartilhamento desabilitado. A retomada deve manter o registro e a intenção `running`, expor falha de ativação sem alterar o arquivo e sem marcar progresso. Depois de liberar o bloqueio, pausa e retomada explícitas limpam a espera e reabrem o mesmo arquivo pela identidade persistida. O teste confirma o conteúdo e a identidade no banco.

O teste exercita bloqueio real do sistema de arquivos e a recuperação pelo serviço. Ainda faltam combinações de bloqueio durante gravações já ativas e disco cheio real.
