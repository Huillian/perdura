# ADR-0002: Formato de mensagens, versão e limites

- **Estado:** aceito
- **Data:** 2026-11-24

## Contexto

As mensagens do protocolo precisam de um formato compacto, padronizado e verificável, com número de versão desde o início.

## Opções consideradas

- CBOR em codificação determinística.
- Formato binário próprio.
- Protocol Buffers.

## Decisão

- Toda mensagem é um array CBOR em codificação determinística (RFC 8949, seção 4.2): inteiros na forma mais curta e comprimentos sempre definidos.
- O primeiro item do array é a versão do protocolo. O segundo é o tipo da mensagem.
- A versão inicial é 0, experimental, sem promessa de compatibilidade.
- Datagramas de descoberta têm no máximo 1200 bytes.
- Mensagens malformadas, de versão desconhecida, de tipo desconhecido, acima do limite ou com bytes sobrando são descartadas em silêncio e contadas em métricas locais.
- A implementação inicial é um codec interno mínimo (inteiros sem sinal, cadeias de bytes e arrays de comprimento definido). Ele pode ser trocado por uma biblioteca, desde que os bytes produzidos sejam idênticos.

## Consequências

- Qualquer biblioteca CBOR consegue ler as mensagens.
- O decodificador estrito precisa de testes negativos e de fuzzing.
- Mudanças de formato na versão 0 não exigem migração.
