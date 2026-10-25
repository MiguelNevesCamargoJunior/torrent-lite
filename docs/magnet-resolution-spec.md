# Resolução assíncrona de magnets

## Contrato

- Um magnet sem metadados entra na fila imediatamente. A busca pela metainfo usa o proxy de rede existente e não segura a trava de mutações do serviço.
- A busca ocupa uma das vagas de `max-active`, para que a soma de buscas e downloads incompletos não ultrapasse o limite. Itens seguintes continuam elegíveis quando houver vagas.
- O motor faz a busca em modo `list_only`: nenhum handle de download ou arquivo é criado antes da validação da metainfo. O resultado passa pela mesma sanitização de `.torrent`, é associado ao hash esperado e salvo no SQLite antes da ativação.
- Pausar ou remover cancela a busca. O término tardio de uma busca cancelada nunca recria um registro, um handle ou arquivos. Retomar agenda nova busca. Parar o serviço cancela todas as buscas.
- Falha ou prazo de 30 segundos preserva o registro e entra no recuo limitado já usado pela fila. A API não revela URL do tracker nem erro interno.
- Metainfo validada em cache continua sendo ativada sem nova busca. O comando de download único aguarda a resolução e a conclusão, enquanto o serviço permanece responsivo.

## Evidência exigida

1. Tracker local que aceita conexões e não responde: `tick_queue`, adição, pausa e remoção retornam em prazo curto; a busca não cria handle nem arquivos.
2. Com limite 1, uma busca reserva a vaga; após pausa, outro torrent inicia. Um resultado atrasado não reativa o magnet.
3. Tracker local e peer de teste fornecem metainfo e conteúdo; o magnet completa, salva metainfo, reinicia e verifica os bytes sem tracker.
4. Erros, prazo e cancelamento mantêm a fila utilizável e não expõem detalhes sensíveis.
