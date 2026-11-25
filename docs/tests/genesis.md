# Genesis Test: registro

Critérios da Fase 1 e a evidência de cada um. Última atualização: 2026-11-24.

## Automatizado, no computador

Ambiente: nós em namespaces de rede Linux isolados, ligados por um par de interfaces virtuais, sem rota para fora.

| Critério | Evidência |
|---|---|
| Descoberta sem cadastro e sem endereço configurado | `scripts/test-discovery.sh` |
| Descoberta quando um dos nós não recebe multicast | `scripts/test-discovery-reply.sh` |
| Identidade permanece após reinício | `scripts/test-identity.sh` |
| Conexão autenticada troca uma mensagem | `scripts/test-channel.sh` |
| Mensagens adulteradas, repetidas ou fora de ordem são rejeitadas | testes unitários do módulo `channel` |
| Nenhum serviço externo é necessário | a rede de teste não tem rota externa |
| Queda do par gera falha explícita, e o retorno restabelece a comunicação | `scripts/test-recovery.sh` |

## Físico, com dois aparelhos Android

- Aparelhos: Motorola moto g(20), com Android 11, e Samsung Galaxy A07 (SM-A075M), com Android 16.
- Programa: o mesmo binário `perdura`, compilado para arm64 e embutido no app de teste em `android/`.

### Roteiro

1. Desligar os dados móveis nos dois aparelhos.
2. Ligar o roteador Wi-Fi (hotspot) em um deles e conectar o outro a essa rede.
3. Abrir o app de teste nos dois e tocar em "Atualizar interfaces".
4. Tocar em "Iniciar" nos dois e aguardar até 20 segundos. A linha `pares` deve indicar 1 nos dois.
5. Enviar uma mensagem de um aparelho. O remetente deve mostrar `send: reply` e o destinatário `listen: received`.
6. Conferir que a impressão digital mostrada após `peer` é a identidade do outro aparelho.
7. Repetir o envio no sentido contrário.
8. Desligar o hotspot e enviar: deve aparecer um erro, sem `reply`. Religar, reconectar e enviar: deve funcionar.

### Resultados em 2026-11-24

| Cenário | Resultado |
|---|---|
| Hotspot do Galaxy A07, com o moto g(20) conectado | Descoberta mútua, mensagem entregue nos dois sentidos, identidades conferidas |
| Hotspot do moto g(20) | Não iniciou: a interface do hotspot não oferece endereço IPv6 link-local ao app |
| Os dois aparelhos no mesmo roteador doméstico, com o binário executado pelo adb | Descoberta mútua, um dos lados somente por resposta direta, e canal nos dois sentidos |
| Os dois aparelhos no mesmo roteador doméstico, pelo app | Inconclusivo: o tráfego entre os aparelhos parou durante o teste |
| Identidade após reiniciar o app | Mantida nos dois aparelhos |

### Pendências

- Executar no aparelho físico o passo 8 do roteiro.
- Caminho por IPv4 para redes sem IPv6 link-local, como o hotspot do moto g(20).
- Proteção da chave de identidade pelo Android Keystore.
- Investigar a perda de tráfego entre aparelhos no roteador doméstico.
