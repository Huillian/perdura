# ADR-0007: Roteamento e retransmissão

- **Estado:** aceito
- **Data:** 2026-11-24

## Contexto

Na Fase 2, um nó precisa alcançar outro que não está ao seu alcance direto, passando por intermediários que podem não ser confiáveis.

## Opções consideradas

- Vetor de distância.
- Inundação com limite de saltos.
- Roteamento na origem, com descoberta de rota sob demanda.

## Decisão

- O roteamento usa vetor de distância. Os destinos são identidades (chaves públicas Ed25519).
- Cada nó anuncia aos vizinhos, somente por canais autenticados, as identidades que alcança e a quantos saltos.
- Proteção contra laços: número de sequência originado pelo destino, limite de 8 saltos e não anunciar uma rota de volta ao vizinho por onde ela foi aprendida.
- Rotas expiram quando deixam de ser anunciadas.
- A tabela de rotas e a frequência dos anúncios têm limites fixos.
- A retransmissão vem desligada por padrão. Um nó só retransmite tráfego de terceiros, e só anuncia rotas para outros destinos, quando o operador a habilita.
- Teto de escala declarado: algumas centenas de nós por malha.

## Consequências

- O nó passa a ser um processo permanente, com canais duradouros entre vizinhos.
- Os vizinhos de um nó ficam sabendo quais identidades ele alcança.
- Na versão 0, os anúncios de rota não são assinados pelo destino. Um vizinho malicioso pode anunciar rotas falsas e atrair ou descartar tráfego. Ele não consegue ler o conteúdo (ADR-0004).
- O algoritmo não escala para milhares de nós. Isso não é meta das Fases 2 a 4.
- Os formatos das mensagens de roteamento serão definidos em `specs/PROTOCOL.md` junto com a implementação.
