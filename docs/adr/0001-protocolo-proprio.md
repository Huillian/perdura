# ADR-0001: Protocolo próprio

- **Estado:** aceito
- **Data:** 2026-11-24

## Contexto

Antes de implementar roteamento, o projeto precisava decidir entre manter um protocolo próprio, adotar o Reticulum ou construir sobre o Yggdrasil. Ao fim da Fase 1, descoberta, identidade e canal seguro estão implementados e validados em aparelhos reais.

## Opções consideradas

- Manter o protocolo próprio.
- Adotar o Reticulum por uma implementação em Rust.
- Usar o Yggdrasil para a conectividade e construir a entrega assíncrona por cima.

## Decisão

- O projeto mantém o protocolo próprio até o fim da Fase 4.
- A decisão será reavaliada antes da Fase 6, quando entram os transportes de rádio de baixa velocidade.

## Justificativa

- O que foi construído na Fase 1 funciona e está coberto por testes. Adotar outra pilha exigiria descartar ou reescrever boa parte.
- Entre as implementações do Reticulum em Rust, a mais completa usa licença AGPL, incompatível com a licença MIT deste projeto, e a que usa MIT está menos madura.
- O ponto forte do Reticulum são enlaces de rádio de baixa velocidade, que este projeto só aborda na Fase 6.

## Consequências

- O roteamento fica sob responsabilidade do projeto. Ele será mantido simples, com teto de escala declarado (ADR-0007).
- O projeto não é interoperável com Reticulum nem com Yggdrasil.
- A especificação pública em `specs/PROTOCOL.md` continua sendo a referência para implementações independentes.
