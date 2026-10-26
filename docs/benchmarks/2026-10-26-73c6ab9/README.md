# Benchmark local: 26 de outubro de 2026

[Workflow e artefatos originais](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/runs/40251256889) · Commit medido: `73c6ab910c4f1327a9d455bb9d203a4ae469bfc3` · [Procedimento](../../benchmark-spec.md)

Três downloads de 256 MiB por sistema, em build release, por TCP loopback. O seeder executou em outro processo. O processo medido usou o armazenamento persistente com SQLite e rechecagem completa dos pedaços. O conteúdo de destino foi comparado byte a byte com a origem em cada execução.

| Runner | Vazão mediana (MiB/s) | CPU mediana do processo (s) | Maior working set amostrado (MiB) |
| --- | ---: | ---: | ---: |
| Windows `win25-vs2026` | 44,64 | 3,078 | 14,81 |
| Linux `ubuntu26` | 213,77 | 1,300 | 20,55 |
| macOS `macos26` | 179,84 | 1,572 | 19,45 |

Dados brutos: [Windows](Windows.json), [Linux](Linux.json), [macOS](macOS.json). A vazão usa somente o intervalo entre adicionar o torrent ao motor e finalizar os pedaços. A CPU acumulada cobre toda a vida do processo medido, incluindo inicialização e comparação final. O working set foi amostrado a cada 100 ms, portanto picos mais curtos podem não aparecer. Os três runners têm hardware e armazenamento distintos; a tabela não isola o efeito do sistema operacional. O teste não usa o proxy de política, a API, DHT, internet pública nem milhares de peers ou arquivos. Esses cenários continuam sem medição.
