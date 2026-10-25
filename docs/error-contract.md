# Erros do serviço local

Toda resposta não bem-sucedida de `/api/v1` usa JSON `{ "error": { "code": "..." } }`, `Content-Type: application/json` e `Cache-Control: no-store`. O código é estável para automação; o status HTTP continua informando a classe do erro. O corpo não ecoa magnet, metainfo, token, URL de tracker ou caminho local.

| HTTP | `code` | Situação |
| --- | --- | --- |
| 400 | `bad_request` | JSON ou codificação inválida |
| 401 | `unauthorized` | token ausente ou errado |
| 403 | `forbidden` | Host ou Origin recusado |
| 404 | `not_found` | registro ou rota ausente |
| 408 | `request_timeout` | corpo de requisição demorou demais |
| 413 | `payload_too_large` | entrada acima do limite |
| 415 | `unsupported_media_type` | tipo de corpo não aceito |
| 422 | `unprocessable` | entrada validada mas não aplicável |
| 429 | `busy` | limite de operações concorrentes |
| 500 | `internal_error` | falha interna do serviço |

O estado de cada torrent inclui métricas somente quando disponíveis. Se uma ativação falhar, o registro permanece com intenção `running`, `observed_state: "retrying"`, `error_code: "activation_failed"` e `retry_after_secs` até a próxima tentativa. A falha não ocupa vaga nem bloqueia os demais itens. O serviço usa backoff de 2 a 60 segundos em memória, crescente por falha; pausa e retomada explícita limpam a espera. O motivo bruto do motor não é enviado à API, porque pode conter caminhos e URLs.
