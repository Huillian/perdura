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
