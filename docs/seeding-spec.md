# Política de compartilhamento inicial

O comando `serve` inicia com `--max-seeding 0`: ao concluir e validar as peças selecionadas, pausa a sessão de rede desse torrent. O operador pode escolher `--max-seeding N` entre 0 e 100. Até N torrents completos com intenção `running` permanecem ativos para enviar peças; os demais ficam pausados, em ordem de cadastro. Um torrent explicitamente pausado nunca compartilha. A soma de `--max-active` e `--max-seeding` não pode exceder 150; o limite de peers por torrent é `min(50, floor(150 / soma))`, restringindo a capacidade configurada a no máximo 150 peers simultâneos.

Vagas de compartilhamento são separadas de `--max-active`, que limita downloads incompletos e resoluções de magnet. Conclusão libera imediatamente a vaga de download; pausa, remoção ou falha liberam a vaga de compartilhamento. `--upload-limit-kib` continua limitando a taxa global. A configuração é do processo, como os limites de banda; reiniciar sem a flag volta ao padrão zero, mas a intenção `running` persistida não muda.

A interface distingue `Compartilhando` de `Concluído` pausado. O contador de bytes enviados vem do motor e pode reiniciar com o processo; não é usado para decidir a política. Esta etapa não promete conexão de entrada pública, ratio, limite de tempo ou disponibilidade garantida de peers. O ouvinte permanece na configuração de rede atual até a qualificação da política de entrada.

## Testes de aceitação

1. Duas transferências locais concluem com `--max-active 1 --max-seeding 1`: a primeira permanece ativa para compartilhar, e a segunda usa a vaga de download liberada.
2. Quando ambas concluem, só a primeira ocupa a vaga de compartilhamento. Pausar a primeira ativa a segunda; retomar a primeira não excede o limite.
3. `--max-seeding 0` mantém o comportamento atual de pausa após conclusão; valores acima de 100 são recusados antes de abrir o serviço.
4. Após reiniciar, as peças são revalidadas antes de qualquer torrent concluído voltar a compartilhar.
