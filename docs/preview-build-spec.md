# Pacotes experimentais de prévia

## Objetivo

Produzir, sob demanda no GitHub Actions, um binário nativo por sistema suportado com `browser-ui,mcp` habilitados. Cada artefato é explicitamente experimental e não constitui uma versão estável. O pacote permite testar CLI, serviço, painel e MCP sem instalar Rust.

## Contrato

- Compilar de `Cargo.lock` com Rust 1.91.1 em `windows-latest`, `ubuntu-latest` e `macos-latest`, perfil release e features `browser-ui,mcp`.
- Executar um smoke test offline de `--help` e `inspect-magnet` no binário produzido.
- Gerar um ZIP com binário, LICENSE, README, manifesto JSON e inventário JSON de dependências. O manifesto registra commit completo, plataforma, features, versão e SHA-256 do binário. O inventário lista os pacotes do grafo do Cargo usado pelo build, com nome, versão, fonte e licença declarada; pode incluir dependências condicionais de outras plataformas e não se apresenta como SBOM completo do binário.
- Publicar ao lado do ZIP um `SHA256SUMS` com seu hash. Nomes e conteúdo de entrada são validados antes de criar o pacote. O ZIP usa nomes ordenados e metadados estáveis para que entradas idênticas produzam bytes idênticos.
- Não publicar uma GitHub Release nem marcar artefatos como assinados. Assinatura, SBOM completo, proveniência atestada e medições de desempenho permanecem gates separados.

## Aceitação

1. Teste local cria executável e metadados sintéticos, verifica arquivos, hash, ordem e determinismo de dois pacotes independentes.
2. Entrada ausente ou plataforma inválida falha antes de escrever um pacote.
3. Workflow manual constrói e executa os três binários, e os três ZIPs têm checksum verificável.
