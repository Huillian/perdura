# ADR-0004: Canal seguro entre dois nós

- **Estado:** aceito
- **Data:** 2026-11-24

## Contexto

Dois nós que se descobriram precisam de um canal autenticado e cifrado, sem que um conheça a chave do outro de antemão.

## Opções consideradas

- Noise Protocol Framework.
- TLS 1.3 com certificados autoassinados.
- QUIC.

## Decisão

- O canal usa `Noise_XX_25519_ChaChaPoly_SHA256`.
- A chave estática do Noise é gerada a cada inicialização e existe só em memória.
- A identidade Ed25519 (ADR-0003) assina a chave estática do Noise. A chave pública de identidade e a assinatura viajam dentro do handshake, já cifradas.
- O transporte é TCP sobre IPv6 link-local. Cada mensagem é precedida do seu tamanho em 2 bytes, em ordem de rede.
- Adulteração e repetição de mensagens são rejeitadas pelo próprio canal.
- A implementação usa a biblioteca `snow`.

## Consequências

- O handshake prova que o par possui a chave que apresenta. Não prova quem ele é. Sem verificação de contatos, um atacante ativo no mesmo enlace pode se interpor no primeiro contato.
- A biblioteca `snow` não passou por auditoria formal. O sistema não deve ser anunciado como seguro para comunicações sensíveis.
- Com dois nós, canal e ponta a ponta coincidem. A proteção de ponta a ponta através de um intermediário será decidida na Fase 2.
- QUIC permanece candidato para o modo Live.

## Emenda de 2026-11-24: sigilo através de intermediários

### Decisão

- Entre origem e destino que não são vizinhos diretos, há uma sessão de ponta a ponta com o mesmo handshake Noise e a mesma prova de identidade do canal entre vizinhos.
- As mensagens dessa sessão viajam dentro dos canais entre vizinhos, como carga que os intermediários repassam sem abrir.
- Entre vizinhos diretos, o canal de enlace já é de ponta a ponta.

### Consequências

- Um intermediário não tem acesso ao conteúdo.
- Um intermediário vê as identidades de origem e de destino, o tamanho e o horário das mensagens que repassa.
