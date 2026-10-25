# Qualificação do painel em navegador

## Contrato

O painel embutido no binário continua sem dependências de front-end em produção. Testes de desenvolvimento executam o HTML real em Chromium, Firefox e WebKit contra um servidor local de fixture. A fixture fornece a mesma forma JSON da API e captura requisições de controle; testes Rust existentes cobrem autenticação, proxy entre UI e serviço, limites de corpo e política CSRF no servidor.

- Uma linha de download em andamento mostra o ETA que a API mediu, em minutos e segundos, quando `eta_secs` é inteiro não negativo. Estado pausado, concluído, aguardando metadados, ou ETA ausente mostram `—`.
- O ETA é apenas uma estimativa com a taxa atual; a página nunca calcula um prazo a partir de dados ausentes.
- Entrar com teclado no campo magnet e pressionar Enter envia a ação. O cabeçalho `X-CSRF-Token` contém o token obtido no endpoint efêmero.
- Abrir Arquivos com teclado mostra o diálogo nativo, e Escape o fecha sem salvar alterações.
- Nomes e caminhos fornecidos pela API permanecem texto, sem execução de HTML.

## Evidência

Testes de browser exercitam a página real e verificam DOM, requisições e foco. A CI instala browsers apenas no job de teste de interface, sem incluí-los no aplicativo distribuído. O contrato do servidor continua coberto pelos testes Rust de `browser_ui`.
