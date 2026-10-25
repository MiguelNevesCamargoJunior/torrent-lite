# Ciclo de vida do serviço local

`torrent-lite serve --state ... --output ...` inicia em primeiro plano e continua sendo o único processo dono do banco. `torrent-lite status --state ...` consulta o serviço pelo endpoint protegido e resume registros, downloads ativos, compartilhamentos ativos e itens aguardando. Serviço ausente retorna erro e código de saída diferente de zero; a consulta não lê diretamente o banco para evitar estado obsoleto.

`torrent-lite stop --state ...` envia `POST /api/v1/service/stop` ao endpoint de loopback com o mesmo Bearer, Host e Origin exigidos pelos demais comandos. A resposta `202 Accepted` confirma que o pedido foi aceito. O processo encerra a sessão do motor, descarta o endpoint protegido e libera o lock do banco. O comando espera o endpoint desaparecer por até 15 segundos e falha se não conseguir confirmar o encerramento. Uma segunda chamada após a parada recebe serviço indisponível; nenhuma operação apaga arquivos baixados.

O servidor MCP oferece `service_status` em modo somente leitura e `service_stop` apenas no modo de escrita. O painel apresenta os contadores e um controle de parada com confirmação; usa o token CSRF existente. A página passa a indicar serviço indisponível após a parada. Toda resposta de erro mantém os códigos genéricos existentes e não revela token, magnet, caminho ou tracker.

## Testes de aceitação

1. Processo iniciado publica endpoint; `status` observa os registros atuais.
2. Pedido sem token ou com Origin indevido é recusado e não para o serviço.
3. Pedido válido recebe `202`, o processo termina com sucesso, o endpoint desaparece e um novo processo abre o mesmo banco sem perda de registros.
4. `service_stop` não aparece no `tools/list` do MCP em modo somente leitura.
5. A parada durante um download conserva a intenção gravada e deixa a recuperação para a rechecagem normal na próxima abertura.
