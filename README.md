# Perdura

> A internet cai. A rede perdura.

Perdura é uma rede de comunicação aberta, privada e descentralizada, operada pelos próprios participantes, que funciona com ou sem a internet convencional.

## Estado

Projeto em fase inicial (Fase 0). Ainda não há rede funcional.

**Não use para comunicações sensíveis.** O código não foi auditado.

## Documentação

- [specs/PROTOCOL.md](specs/PROTOCOL.md): especificação do protocolo.
- [docs/adr/](docs/adr/): decisões arquiteturais.

## Desenvolvimento

Requer Rust estável.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Teste de descoberta entre dois nós em redes isoladas (Linux, sem root):

```sh
cargo build -p perdura-cli
./scripts/test-discovery.sh
```

Teste de persistência da identidade:

```sh
cargo build -p perdura-cli
./scripts/test-identity.sh
```

Teste completo entre dois nós: descoberta, canal cifrado e troca de mensagem:

```sh
cargo build -p perdura-cli
./scripts/test-channel.sh
```

Teste de falha explícita e reconexão:

    cargo build -p perdura-cli
    ./scripts/test-recovery.sh

Teste de descoberta com um nó que não recebe multicast:

    cargo build -p perdura-cli
    ./scripts/test-discovery-reply.sh

## Licença

MIT. Veja [LICENSE](LICENSE).
