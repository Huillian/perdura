# ADR-0005: Descoberta local e privacidade dos anúncios

- **Estado:** aceito
- **Data:** 2026-11-24

## Contexto

Dois nós no mesmo enlace precisam se encontrar sem servidor, sem DHCP e sem endereço configurado à mão, expondo o mínimo de metadados.

## Opções consideradas

- Multicast UDP em IPv6 link-local, com anúncio próprio.
- mDNS/DNS-SD do sistema operacional.
- Broadcast IPv4.

## Decisão

- Anúncios por multicast UDP em IPv6 link-local.
- Grupo `ff12::7065:7264` e porta `29170`. Ambos são provisórios.
- O anúncio contém a versão, o tipo, um identificador de nó de 16 bytes aleatórios e uma porta.
- O identificador é gerado a cada inicialização. A chave pública nunca é anunciada.
- Um anúncio a cada 5 segundos, com variação aleatória de até 0,5 segundo para mais ou para menos.
- Um par é removido após 15 segundos sem anúncio.
- Só são aceitos anúncios com origem em `fe80::/10`. O nó ignora o próprio identificador.
- A tabela guarda no máximo 256 pares.
- Na versão 0, o campo de porta leva a própria porta de descoberta. Nenhum serviço de conexão escuta ainda.

## Consequências

- A descoberta não autentica ninguém. Qualquer participante do enlace pode forjar anúncios. A identidade só é estabelecida na conexão autenticada (ADR-0004).
- Alguns aparelhos Android filtram multicast IPv6 no Wi-Fi. Se o teste físico falhar por isso, um anúncio por broadcast IPv4 será avaliado em novo ADR.

## Emenda de 2026-11-24: resposta direta (em teste)

### Contexto

A sonda física mostrou que a recepção de multicast varia entre aparelhos. Um celular recebeu os anúncios do outro, mas o outro não recebeu nenhum. Com isso, só um dos lados descobria o par.

### Decisão

- Ao receber um anúncio (tipo 1) válido, o nó envia uma resposta direta (tipo 4) em unicast para o endereço de origem do anúncio.
- A resposta direta tem os mesmos campos do anúncio.
- Uma resposta direta nunca é respondida. Isso impede laços.
- O nó envia no máximo uma resposta direta a cada 4 segundos para cada par, e nenhuma para pares que não couberam na tabela.
- Quem recebe uma resposta direta registra o par como se tivesse recebido um anúncio.

### Consequências

- Basta um dos dois lados receber multicast para os dois se descobrirem.
- Quando os dois lados recebem multicast, o tráfego de descoberta por par dobra.
- Um participante do enlace pode induzir respostas diretas forjando anúncios. O efeito é limitado pelo tamanho igual das mensagens, pelo limite de frequência e pela tabela de pares.
- A decisão é experimental e será reavaliada após os testes físicos.
