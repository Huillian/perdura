# Protocolo Perdura

Versão 0. Experimental. Sem promessa de compatibilidade.

## 1. Codificação

Toda mensagem é um array CBOR em codificação determinística (RFC 8949, seção 4.2).

- Item 0: versão do protocolo (inteiro sem sinal). Nesta especificação, `0`.
- Item 1: tipo da mensagem (inteiro sem sinal).

O receptor descarta em silêncio qualquer mensagem que não siga esta especificação, incluindo inteiros fora da forma mais curta, comprimentos indefinidos e bytes após o fim do array.

## 2. Descoberta

- Transporte: UDP sobre IPv6.
- Grupo multicast: `ff12::7065:7264` (escopo de enlace).
- Porta: `29170`.
- Tamanho máximo do datagrama: 1200 bytes.
- O receptor só aceita datagramas com origem em `fe80::/10`.

### 2.1 Anúncio (tipo 1)

`[0, 1, node_id, port]`

| Item | Tipo CBOR | Conteúdo |
|---|---|---|
| 0 | inteiro sem sinal | versão, `0` |
| 1 | inteiro sem sinal | tipo, `1` |
| 2 | cadeia de bytes, 16 bytes | identificador aleatório, novo a cada inicialização |
| 3 | inteiro sem sinal, até 65535 | porta de contato |

Intervalo de envio: 5 segundos, com variação aleatória de até 0,5 segundo. Um par é considerado perdido após 15 segundos sem anúncio.

### 2.2 Vetor de teste

`node_id` com 16 bytes `0x11` e `port` igual a 29170:

`84000150111111111111111111111111111111111971f2`

## 3. Identidade

- A identidade de um nó é uma chave pública Ed25519 de 32 bytes.
- A impressão digital é essa chave em base32 (RFC 4648), em minúsculas e sem preenchimento, o que dá 52 caracteres, escritos em 13 grupos de 4 separados por hífen.

### 3.1 Vetores de teste

| Chave pública | Impressão digital |
|---|---|
| 32 bytes `0x00` | `aaaa-aaaa-aaaa-aaaa-aaaa-aaaa-aaaa-aaaa-aaaa-aaaa-aaaa-aaaa-aaaa` |
| 32 bytes `0xff` | `7777-7777-7777-7777-7777-7777-7777-7777-7777-7777-7777-777q` |

## 4. Canal seguro

- Transporte: TCP sobre IPv6 link-local, porta `29170`. O nó recusa pares cujo endereço não esteja em `fe80::/10`.
- Enquadramento: cada mensagem Noise é precedida do seu tamanho em 2 bytes, em ordem de rede.
- Handshake: `Noise_XX_25519_ChaChaPoly_SHA256`, com prólogo `perdura/0` (9 bytes ASCII).
- A chave estática do Noise é gerada a cada inicialização.
- Depois de qualquer erro, a conexão é encerrada.

### 4.1 Prova de identidade (tipo 2)

`[0, 2, identity_key, signature]`

| Item | Tipo CBOR | Conteúdo |
|---|---|---|
| 0 | inteiro sem sinal | versão, `0` |
| 1 | inteiro sem sinal | tipo, `2` |
| 2 | cadeia de bytes, 32 bytes | chave pública de identidade (Ed25519) |
| 3 | cadeia de bytes, 64 bytes | assinatura Ed25519 |

A assinatura cobre os 23 bytes ASCII `perdura-noise-static-v0` seguidos dos 32 bytes da chave estática Noise de quem assina.

- A primeira mensagem do handshake leva carga vazia.
- A segunda leva a prova do respondedor. A terceira leva a prova do iniciador.
- Quem recebe a prova verifica a assinatura contra a chave estática Noise que o par apresentou no handshake. Se a verificação falhar, a conexão é encerrada.

### 4.2 Dados (tipo 3)

`[0, 3, payload]`

| Item | Tipo CBOR | Conteúdo |
|---|---|---|
| 0 | inteiro sem sinal | versão, `0` |
| 1 | inteiro sem sinal | tipo, `3` |
| 2 | cadeia de bytes, até 16384 bytes | conteúdo da aplicação |

Vetor de teste: o conteúdo ASCII `hi` é codificado como `830003426869` antes de ser cifrado.
