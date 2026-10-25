# Limites globais de banda

`serve` aceita `--download-limit-kib` e `--upload-limit-kib`, em KiB por segundo. Ausência da opção mantém a respectiva direção sem limite artificial. Os valores são aplicados aos limitadores globais da sessão de rede, portanto abrangem todos os torrents ativos. São configuração do processo: o operador deve repetir as opções ao iniciar o serviço novamente.

Valores entre 16 e 4.194.303 KiB/s são aceitos. A unidade mínima equivale a um bloco BitTorrent de 16 KiB; valores menores fariam o limitador do motor rejeitar blocos inteiros. Entradas zero, menores que 16 ou que excedam o intervalo representável falham antes de iniciar a sessão. O limite é uma média com rajadas de até aproximadamente um segundo, não uma taxa instantânea rígida.

Testes: validação de limites e transbordamento, configuração efetiva da sessão e transferência local com limite de download. Medições comparativas de throughput, CPU e memória em Windows, macOS e Linux continuam sendo gate de release estável.
