# Benchmark do caminho de serviço

## Objetivo

Complementar o benchmark direto do engine com uma medição do caminho real de serviço: SQLite, armazenamento persistente com identidades, rechecagem completa, proxy de política, API autenticada e cliente HTTP local. O benchmark existente continua como uma referência menor e seus resultados históricos não são misturados com os novos.

## Ambiente e limites

- Usar somente uma carga sintética determinística, com padrão de 256 MiB e blocos de 1 MiB. Um processo seeder separado fornece os dados; ele não entra nas amostras de memória e CPU do processo medido.
- Um tracker HTTP de teste e o seeder escutam apenas em loopback. O torrent usa o tracker local para descobrir o peer; tracker e peer devem passar pelo proxy de política. O benchmark falha se não observar ao menos uma conexão proxied para cada rota.
- O serviço do teste usa `DestinationPolicy::AllowLan` exclusivamente para que as fixtures loopback sejam permitidas. O serviço distribuído continua em `PublicOnly`; este resultado não qualifica tráfego para a internet nem altera o padrão de rede do aplicativo.
- A transferência é registrada por uma API autenticada real, usando endpoint e token publicados pelo mecanismo de descoberta existente. O benchmark não chama métodos internos para adicionar o torrent ou consultar progresso.
- O processo medido inclui `ServiceCore`, armazenamento, proxy, API e o cliente de controle local. A fixture seeder é excluída. A interface de navegador não é aberta e sua memória permanece fora desta medição.

## Métricas e verificação

1. Começar o relógio imediatamente antes do `POST` autenticado que adiciona o `.torrent` e parar quando a API informar que o item concluiu.
2. Registrar bytes transferidos, tempo, MiB/s, tempo de CPU do processo, maior working set amostrado a cada 100 ms, contagem de conexões pelo proxy e anúncios atendidos pelo tracker.
3. Parar o serviço pela API autenticada, reabrir o mesmo banco e diretório e aguardar a rechecagem completa do engine. Registrar separadamente o tempo de recuperação/verificação.
4. Comparar byte a byte o arquivo recuperado com a origem. Nenhum resultado é válido se a comparação, a rechecagem ou as duas rotas proxied falharem.
5. Emitir JSON versionado por execução e resumir três execuções por sistema. Incluir commit, sistema/imagem do runner, tamanho, transporte, política de fixture e amostras brutas.

## Interpretação

O resultado mede o caminho local de serviço e confirma a presença do proxy/API no caminho de dados. Não mede internet pública, DHT, UDP, latência de trackers externos, múltiplos peers ou torrents com muitos arquivos. Working set amostrado pode perder picos menores que 100 ms. Os runners têm hardware distinto; não atribuir diferenças somente ao sistema operacional. Não publicar alegação de velocidade universal ou meta de memória a partir deste cenário sintético.

## Gates

- O teste de benchmark deve falhar se a transferência não passar pela API e pelas duas rotas do proxy.
- Deve falhar se a API marcar conclusão sem que os bytes persistidos correspondam à origem.
- A retomada deve validar os mesmos dados sem depender do tracker ou do seeder.
- Execução manual gera JSON separado para Windows, Linux e macOS; os resultados e a revisão de regressões ficam registrados antes de alterar metas de recursos.
