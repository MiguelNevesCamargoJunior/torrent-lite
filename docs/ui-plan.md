# Interface local de acompanhamento

Estado: interface local experimental, 2026-10-25.

Comando `torrent-lite ui --state <state.db>`: inicia um pequeno servidor HTTP em `127.0.0.1` numa porta dinâmica e apresenta uma URL local para abrir no navegador. A interface lê o serviço ativo por `LocalClient`, sem acesso direto ao banco ou aos arquivos baixados. O processo da interface pode ser encerrado sem interromper downloads.

O servidor da interface mantém um segredo aleatório efêmero de 128 bits no caminho da URL. Esse segredo é distinto do token de controle do serviço e morre ao encerrar `ui`. Cada requisição verifica `Host` e o caminho completo; requisições JSON verificam `Origin` quando presente. Respostas usam `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, `X-Content-Type-Options: nosniff` e CSP restritiva. Não há recursos externos, cookies, anúncios ou telemetria. A página exibe nomes dos torrents, hashes e métricas, sem caminhos locais ou URLs de trackers. O risco residual de exposição da URL no histórico do navegador será documentado antes de marcar a interface estável.

Lista e progresso atualizados a cada dois segundos, estados acessíveis por texto e barras de progresso, indicação de serviço desconectado. A etapa de controle permite adicionar magnet e `.torrent`, pausar, retomar, verificar e remover registro (sem apagar arquivos). HTML/CSS/JS pequenos, empacotados no binário, sem framework de front-end ou processo de build separado.

O navegador só conversa com o processo `ui`, que retransmite ações ao `LocalClient`; o bearer privado do serviço nunca entra na página ou nas respostas. `GET /{segredo}/csrf` fornece um segundo segredo efêmero de 256 bits, diferente do segredo da URL. Cada POST exige Host exato, caminho secreto, Origin exato e `X-CSRF-Token` exato. Corpo JSON ou `.torrent` exige Content-Type específico e limite de tamanho; outros formatos e verbos são recusados. O cliente usa `fetch` de mesma origem e envia o token em cabeçalho customizado. Não há CORS. Erros não incluem entradas, caminhos ou tokens. A ausência de Origin em POST é recusada.

Direção visual: painel de transferências com linhas separadas, fundo grafite, acentos ciano para atividade e âmbar para pausa. Tipografia do sistema, números tabulares, adaptação a telas estreitas e respeito a movimento reduzido. Nomes vindos do torrent são inseridos como texto, sem interpretar HTML.

Testes: servidor bind apenas em loopback, caminho e Host incorretos recusados, sem segredo de controle na resposta, lista sintética renderizada, CSP e cabeçalhos de cache presentes. Medir tamanho do binário e RSS nas três plataformas antes de uma versão estável.
