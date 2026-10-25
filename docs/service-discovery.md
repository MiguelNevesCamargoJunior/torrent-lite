# Descoberta local do serviço

Estado: implementação em andamento, 2026-10-25. Este contrato conecta o processo dono do banco à CLI, ao MCP stdio e ao painel sem colocar credenciais em argumentos ou URLs.

O processo que já possui o lock de `state.db` inicia a API em `127.0.0.1:0`, gera um token aleatório de 256 bits e publica `state.db.endpoint`. O arquivo contém versão, porta, geração aleatória e token protegido. No Windows, o token é cifrado com DPAPI para o usuário atual, com UI proibida e sem escopo `LOCAL_MACHINE`. Em Unix, o arquivo é criado com modo `0600` antes de receber dados; a leitura recusa permissões para grupo ou outros. O conteúdo tem limite de 4 KiB. A escrita usa arquivo temporário `create_new`, sincronização e rename. Uma instância nova substitui um endpoint antigo somente depois de adquirir o lock do banco. Um endpoint obsoleto nunca autoriza acesso sozinho: a API exige o token que está em memória da instância atual.

Os clientes validam versão e porta, recuperam o token sob a proteção do SO e usam `Authorization: Bearer` em pedidos ao Host exato `127.0.0.1:<porta>`. Nunca enviam o token em query string, logs, argumentos ou HTML. O arquivo de descoberta é removido no shutdown normal somente se a geração ainda coincidir. Após queda abrupta, a próxima instância sobrescreve o endpoint obsoleto; clientes tratam conexão recusada como serviço indisponível.

Testes: publicar/ler token e porta; rejeitar arquivo grande, versão inválida, porta fora do loopback, token adulterado e permissões Unix abertas; verificar que o token não aparece em texto claro no disco Windows. O teste de processo deve validar reconexão após encerramento forçado. A descoberta não substitui os testes de autorização, Host e Origin da API.

Referências Windows: [CryptProtectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata) e [CryptUnprotectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata).
