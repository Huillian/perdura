# ADR-0003: Identidade (versão mínima)

- **Estado:** aceito
- **Data:** 2026-11-24

## Contexto

Cada nó precisa de uma identidade estável, criada e guardada no próprio aparelho, sem conta, telefone ou servidor.

## Opções consideradas

- Par de chaves Ed25519 como identidade.
- Usar diretamente a chave do canal cifrado como identidade.
- Identidade derivada de frase de recuperação.

## Decisão

- A identidade é um par de chaves Ed25519 gerado no aparelho. A chave pública é a identidade.
- A impressão digital é a chave pública em base32 (RFC 4648, minúsculas, sem preenchimento), em grupos de quatro caracteres separados por hífen.
- No computador, a chave privada fica em um arquivo com permissão 0600, dentro de um diretório com permissão 0700. O nó recusa um arquivo legível por outros usuários.
- Um arquivo de identidade inválido nunca é sobrescrito. O nó falha com erro explícito.
- No Android, a chave será cifrada por uma chave do Keystore. Isso será implementado junto com o app.
- Não há recuperação na versão 0. Perder o arquivo significa ter outra identidade.
- O identificador efêmero da descoberta (ADR-0005) não tem relação com a identidade.

## Consequências

- A identidade pode assinar objetos, o que a entrega assíncrona vai exigir.
- No computador, quem tiver acesso à conta do usuário pode copiar a chave. Proteção por senha fica para um ADR futuro.
- Exportação, importação, troca e revogação de chave continuam em aberto.
- Verificação de contatos entre pessoas fica para a fase do app.
