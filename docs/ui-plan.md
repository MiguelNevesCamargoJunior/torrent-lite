# Interface local de acompanhamento

Estado: desenho da primeira interface, 2026-10-25.

Comando `torrent-lite ui --state <state.db>`: inicia um pequeno servidor HTTP em `127.0.0.1` numa porta dinâmica e apresenta uma URL local para abrir no navegador. A interface lê o serviço ativo por `LocalClient`, sem acesso direto ao banco ou aos arquivos baixados. O processo da interface pode ser encerrado sem interromper downloads.

O servidor da interface mantém um segredo aleatório efêmero de 128 bits no caminho da URL. Esse segredo é distinto do token de controle do serviço e morre ao encerrar `ui`. Cada requisição verifica `Host` e o caminho completo; requisições JSON verificam `Origin` quando presente. Respostas usam `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, `X-Content-Type-Options: nosniff` e CSP restritiva. Não há recursos externos, cookies, anúncios ou telemetria. A página exibe nomes dos torrents, hashes e métricas, sem caminhos locais ou URLs de trackers. O risco residual de exposição da URL no histórico do navegador será documentado antes de marcar a interface estável.

Primeira etapa somente de leitura: lista e progresso atualizados a cada dois segundos, estados acessíveis por texto e barras de progresso, indicação de serviço desconectado. Ações de pausar, retomar, adicionar e remover entram após teste de proteção contra requisições de outros sites. HTML/CSS/JS pequenos, empacotados no binário, sem framework de front-end ou processo de build separado.

Direção visual: painel de transferências com linhas separadas, fundo grafite, acentos ciano para atividade e âmbar para pausa. Tipografia do sistema, números tabulares, adaptação a telas estreitas e respeito a movimento reduzido. Nomes vindos do torrent são inseridos como texto, sem interpretar HTML.

Testes: servidor bind apenas em loopback, caminho e Host incorretos recusados, sem segredo de controle na resposta, lista sintética renderizada, CSP e cabeçalhos de cache presentes. Medir tamanho do binário e RSS nas três plataformas antes de uma versão estável.
