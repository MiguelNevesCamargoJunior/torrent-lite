# Benchmark local reproduzível

O benchmark cria um arquivo determinístico de 256 MiB por padrão, em blocos de 1 MiB para não manter todo o conteúdo na memória. Um subprocesso seeder serve o `.torrent` somente em loopback. O processo medido usa o armazenamento persistente real, banco SQLite, rechecagem sem fastresume e conexão direta com esse peer. O relógio começa antes de adicionar o torrent ao motor e para quando todos os pedaços terminam. Depois disso, o benchmark compara os arquivos byte a byte.

O script executa o binário de benchmark em perfil release três vezes por sistema. A cada 100 ms, amostra o working set do processo que baixa e o tempo de CPU acumulado desse processo. Registra cada execução, a mediana da vazão e do tempo de CPU e o maior working set observado. O seeder gera o arquivo de origem em outro processo e não entra nessa amostra. A CPU do processo medido inclui inicialização e a comparação final dos arquivos, além da transferência. A CI manual gera artefatos JSON separados para Windows, Linux e macOS, contendo sistema, commit, tamanho, método e resultados.

Isto mede uma transferência local sintética e não estima velocidade de internet ou uso de memória em torrents com milhares de arquivos e peers. O working set amostrado pode perder picos mais curtos que 100 ms. Essas limitações devem acompanhar qualquer número publicado.
