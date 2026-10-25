# Clientes simultâneos da API local

O serviço deve aceitar requisições simultâneas autenticadas sem criar registros duplicados para o mesmo info hash. Dezesseis conexões independentes cadastram magnets pausados: oito usam o mesmo info hash e oito usam hashes distintos. Todas devem receber sucesso; as respostas do grupo duplicado devem conter o mesmo ID. A lista e o banco reaberto devem conter nove registros.

O teste usa sockets HTTP reais e roda na matriz de Windows, Linux e macOS. Esta etapa verifica o cadastro concorrente e a persistência. Ainda faltam corridas entre operações distintas, como pausa, seleção e remoção, além de limites sob carga sustentada.
