# Benchmark do serviço: 26 de outubro de 2026

[Workflow e artefatos originais](https://github.com/MiguelNevesCamargoJunior/torrent-lite/actions/runs/40264280101) · Commit medido: `cfb0558ab527f5471b431fd482790d15e70bc893` · [Procedimento](../../service-benchmark-spec.md)

Três execuções de um payload determinístico de 256 MiB por runner, em build release. O caminho medido inclui serviço, SQLite, armazenamento persistente, proxy de política, API autenticada e cliente local. Tracker e peer de teste passaram pelo proxy; cada execução confirmou as duas conexões, comparou os bytes baixados com a origem e repetiu a validação completa após reabrir o serviço sem tracker ou seeder.

| Runner | Vazão mediana (MiB/s) | CPU mediana do processo (s) | Mediana do maior working set amostrado (MiB) | Rechecagem após reabrir (mediana, s) |
| --- | ---: | ---: | ---: | ---: |
| Windows `win25-vs2026` | 49,960 | 3,703 | 16,64 | 0,199 |
| Linux `ubuntu26` | 270,009 | 1,240 | 17,88 | 0,157 |
| macOS `macos26` | 296,589 | 1,357 | 20,57 | 0,163 |

Dados brutos: [Windows](Windows.json), [Linux](Linux.json), [macOS](macOS.json). Os nove downloads e as nove rechecagens passaram; em todos os casos a reabertura não dependeu do tracker nem do seeder. A vazão cobre o intervalo do pedido autenticado de adição até a conclusão dos pedaços. O tempo de CPU cobre o processo medido, inclusive inicialização, transferência e comparação final. O working set máximo foi amostrado a cada 100 ms e pode perder picos mais curtos. A origem do payload e o navegador ficam fora da amostra de recursos.

O teste usa peers sintéticos em loopback e a política `AllowLan` somente na fixture. Não mede internet pública, DHT, UDP, swarm com múltiplos peers nem torrents com muitos arquivos. Os runners têm hardware e armazenamento distintos; os números não isolam o efeito do sistema operacional nem representam uma promessa de velocidade ou memória para outros cenários.
