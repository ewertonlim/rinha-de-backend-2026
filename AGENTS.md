# AGENTS.md — Guia de Build e Teste

Este documento fornece as instruções necessárias para agentes de IA ou desenvolvedores entenderem como buildar e testar este projeto de **detecção de fraude** da Rinha de Backend 2026.

---

## Visão Geral da Arquitetura

```
nginx (porta 9999, 0.10 CPU, 30MB)
    ├── api1 (porta 8080, 0.45 CPU, 160MB)
    └── api2 (porta 8080, 0.45 CPU, 160MB)
```

A API é escrita em **Rust** com **Actix-Web**. O núcleo de detecção é um motor **KNN (K-Nearest Neighbors)** que:

1. Vetoriza a transação em 14 dimensões (normalizadas para `i16` × 10.000)
2. Busca os 5 vizinhos mais próximos usando instruções SIMD **AVX2** (`_mm256_madd_epi16`)
3. Decide `approved: true/false` com base na maioria dos vizinhos

O índice binário (`index.bin`) é construído offline durante o `docker build`:
- Lê 3.000.000 registros de referência de `resources/references.json.gz`
- Normaliza os vetores para `i16` (valores 0–10000)
- Ordena por soma (`sum`) para permitir busca por vizinhança de forma eficiente
- Armazena vetores e labels separados no arquivo binário

### Formato do arquivo `index.bin`
```
[IndexHeader 16 bytes] + [ReferenceRecord × 3.000.000 × 32 bytes] + [labels × 3.000.000 × 1 byte]
```

---

## Pré-requisitos

- **Docker Desktop** com suporte a containers Linux
- **Docker Compose** v2+
- CPU com suporte a **AVX2** (Intel Haswell+ / AMD Ryzen+)
- O repositório de testes oficial em: `C:\Users\ewert\workspace\rinha-test\official_repo\test\`
  - Contém: `test.js`, `smoke.js`, `test-data.json`, `docker-compose.yml`

---

## Build e Deploy

### 1. Subir a API (com rebuild completo)

```powershell
cd C:\Users\ewert\workspace\rinha
docker compose up --build -d
```

Este comando:
- Compila os binários Rust com `RUSTFLAGS="-C target-cpu=x86-64-v2"` (habilita AVX2)
- Executa `build_index` que lê `references.json.gz` e gera `index.bin`
- Sobe dois containers `api1` e `api2` + `nginx`

> **Tempo esperado:** ~30 segundos para compilação + ~4 segundos para geração do índice

### 2. Verificar se a API está saudável

```powershell
docker compose ps
```

Todos os containers devem estar `healthy`. Verificar manualmente:

```powershell
curl http://localhost:9999/ready
# Deve retornar HTTP 200
```

### 3. Testar um payload manualmente

```powershell
curl -X POST http://localhost:9999/fraud-score `
  -H "Content-Type: application/json" `
  -d (Get-Content C:\Users\ewert\workspace\rinha\payload.json -Raw)
```

Resposta esperada:
```json
{"approved": true, "fraud_score": 0.2}
```

---

## Testes Oficiais

Os testes usam **k6** via Docker Compose do repositório oficial.

### Smoke Test (rápido, ~30s)

```powershell
cd C:\Users\ewert\workspace\rinha-test\official_repo\test
docker compose --profile smoke up
```

### Teste Completo (2 minutos, 250 VUs)

```powershell
cd C:\Users\ewert\workspace\rinha-test\official_repo\test
docker compose --profile test up
```

### Ler o resultado

```powershell
Get-Content C:\Users\ewert\workspace\rinha-test\official_repo\test\results.json
```

---

## Métricas de Avaliação

O score final é calculado com base em:

| Métrica | Descrição |
|---|---|
| `tp_count` | True Positives (fraudes detectadas corretamente) |
| `tn_count` | True Negatives (transações legítimas aprovadas) |
| `fp_count` | False Positives (legítimas bloqueadas erroneamente) — penalidade x2 |
| `fn_count` | False Negatives (fraudes não detectadas) — penalidade x3 |
| `p99` | Latência P99 em ms — afeta diretamente o score |

Fórmula simplificada:
- `E = fp_count * 2 + fn_count * 3 + http_errors * 10`
- `detection_score = taxa_de_acerto × 5000 - penalidade_absoluta`
- `p99_score = 2500 - (p99_ms - 10) × 5` (zerado se p99 > 510ms)
- `final_score = detection_score + p99_score`

**Meta:** `final_score > 4000`


## Estrutura do Código

```
src/
├── main.rs          # Entry point Actix-Web, carrega o índice e serve /fraud-score
├── models.rs        # Structs: FraudRequest, FraudResponse, ReferenceRecord ([i16; 16])
├── vectorize.rs     # Transforma FraudRequest em vetor [i16; 16] normalizado
├── search.rs        # Motor KNN via SIMD AVX2 (_mm256_madd_epi16)
├── index.rs         # Carrega index.bin via mmap
├── decision.rs      # Decide approved/fraud_score a partir do resultado KNN
└── bin/
    └── build_index.rs  # CLI que gera index.bin a partir de references.json.gz
resources/
├── references.json.gz   # 3M registros de referência (fraud/legit)
├── normalization.json   # Valores máximos para normalização dos vetores
└── mcc_risk.json        # Score de risco por MCC (código de categoria do merchant)
```

---

### `docker-compose.yml` — limites de recursos

| Serviço | CPUs | Memória |
|---|---|---|
| nginx | 0.10 | 30MB |
| api1 | 0.45 | 160MB |
| api2 | 0.45 | 160MB |

> **Importante:** O total de recursos deve ser ≤ 1.0 CPU e ≤ 350MB RAM conforme as regras da Rinha.

---

## Fluxo de Diagnóstico

Se os testes falharem com timeouts:

```powershell
# 1. Ver logs da API
docker logs rinha-api1-1
docker logs rinha-api2-1

# 2. Testar conectividade manualmente
curl http://localhost:9999/ready

# 3. Verificar uso de recursos durante o teste
docker stats

# 4. Checar se o índice foi gerado corretamente no build
docker compose up --build -d 2>&1 | Select-String "index.bin"
```

---

## Workflow Recomendado para Melhorias

1. Buildar: `docker compose up --build -d` (no diretório `rinha`)
2. Aguardar containers ficarem `healthy`
3. Rodar teste completo e ler `results.json`
4. Comparar `final_score` com baseline anterior

> **Dica:** Reduza `250_000` para `50_000` se P99 estiver alto. Aumente se a precisão for baixa.

---

## Dicas de Otimização

As seguintes dicas essenciais de performance já estão implementadas neste projeto:

- **Faça o pré-processamento das 3 milhões de referências:** Deixa elas num formato binário (`index.bin`) e builda a imagem docker com elas. (Implementado via `build_index` e multi-stage no `Dockerfile`).
- **Use SIMD (single instruction, multi data) onde der:** O projeto utiliza SIMD com instruções AVX2 (`_mm256_madd_epi16`, etc.) para o cálculo de distância euclidiana.
- **Comece com VP tree ou IVF para busca vetorial – não use busca por força bruta:** A busca vetorial utiliza um índice **IVF (Inverted File Index)** com *zero heap allocations*, evitando a busca por força bruta.
- **Instrumente cada parte do seu código e entenda onde está o gargalo. Arrume uma coisa de cada vez:** O arquivo `main.rs` instrumenta as etapas de `vectorize_us`, `search_us` e `decide_us`, logando essas métricas periodicamente.

---

# Regras de detecção de fraude

Este documento define como a sua API deve transformar uma transação em um vetor de detecção de fraude. Ele cobre a vetorização (as 14 dimensões) e as regras de normalização. A busca vetorial usa esse vetor para encontrar, no dataset de referência, as 5 transações mais parecidas com a que acabou de chegar e, a partir daí, decidir se a nova transação é fraudulenta.

Se você ainda não conhece o conceito de busca vetorial, vale a pena começar por [BUSCA_VETORIAL.md](./BUSCA_VETORIAL.md) — lá o assunto é apresentado de forma didática, com um exemplo bem simplificado.


## Visão geral do fluxo

O fluxo abaixo mostra, com um exemplo real da Rinha de Backend de transação legítima, o passo a passo que a sua API deve fazer para decidir sobre uma transação. Neste caso, um cliente faz uma compra de baixo valor em um comerciante que ele já conhece, perto de casa.

```
1. recebe a requisição:
    {
      "id": "tx-1329056812",
      "transaction":      { "amount": 41.12, "installments": 2, "requested_at": "2026-03-11T18:45:53Z" },
      "customer":         { "avg_amount": 82.24, "tx_count_24h": 3, "known_merchants": ["MERC-003", "MERC-016"] },
      "merchant":         { "id": "MERC-016", "mcc": "5411", "avg_amount": 60.25 },
      "terminal":         { "is_online": false, "card_present": true, "km_from_home": 29.23 },
      "last_transaction": null
    }
          ↓
2. vetoriza e normaliza (14 dimensões):
    [0.0041, 0.1667, 0.05, 0.7826, 0.3333, -1, -1, 0.0292, 0.15, 0, 1, 0, 0.15, 0.006]
          ↓
3. busca os 5 vizinhos mais próximos (ex.: distância euclidiana):
    dist=0.0340  legit
    dist=0.0488  legit
    dist=0.0509  legit
    dist=0.0591  legit
    dist=0.0592  legit
          ↓
4. calcula o score (threshold 0.6):
    score = 0 fraudes / 5 = 0.0
    approved = score < 0.6 → true
          ↓
5. resposta:
    {
      "approved": true,
      "fraud_score": 0.0
    }
```

Repare nos `-1` nas posições 5 e 6: como `last_transaction` veio como `null`, não há "minutos desde a última transação" nem "km desde a última transação" para normalizar.

## As 14 dimensões do vetor

As transações ([exemplos realistas aqui](/resources/example-payloads.json)) precisam ser transformadas em vetores de 14 posições, seguindo a ordem e as regras de normalização abaixo.

| índice | dimensão                 | fórmula                                                                          |
|-----|--------------------------|----------------------------------------------------------------------------------|
| 0   | `amount`                 | `limitar(transaction.amount / max_amount)`                                         |
| 1   | `installments`           | `limitar(transaction.installments / max_installments)`                             |
| 2   | `amount_vs_avg`          | `limitar((transaction.amount / customer.avg_amount) / amount_vs_avg_ratio)`        |
| 3   | `hour_of_day`            | `hora(transaction.requested_at) / 23`  (0-23, UTC)                               |
| 4   | `day_of_week`            | `dia_da_semana(transaction.requested_at) / 6`    (seg=0, dom=6)                  |
| 5   | `minutes_since_last_tx`  | `limitar(minutos / max_minutes)` ou `-1` se `last_transaction: null`             |
| 6   | `km_from_last_tx`        | `limitar(last_transaction.km_from_current / max_km)` ou `-1` se `last_transaction: null` |
| 7   | `km_from_home`           | `limitar(terminal.km_from_home / max_km)`                                          |
| 8   | `tx_count_24h`           | `limitar(customer.tx_count_24h / max_tx_count_24h)`                                |
| 9   | `is_online`              | `1` se `terminal.is_online`, senão `0`                                           |
| 10  | `card_present`           | `1` se `terminal.card_present`, senão `0`                                        |
| 11  | `unknown_merchant`       | `1` se `merchant.id` não estiver em `customer.known_merchants`, senão `0` (invertido: `1` = desconhecido) |
| 12  | `mcc_risk`               | `mcc_risk.json[merchant.mcc]` (valor padrão `0.5`)                               |
| 13  | `merchant_avg_amount`    | `limitar(merchant.avg_amount / max_merchant_avg_amount)`                           |

A função `limitar(x)` mantém o valor dentro do intervalo `[0.0, 1.0]` — é o que se costuma chamar de *clamp*: tudo que fica abaixo de `0.0` vira `0.0`, e tudo que passa de `1.0` vira `1.0`.

### O caso especial do `last_transaction: null`

Os índices 5 e 6 dependem da transação anterior do cliente. Quando a transação atual é a primeira do cliente (ou seja, `last_transaction` vem como `null` no payload), não existe valor a normalizar. Nesses casos, a sua API deve usar o valor sentinela `-1` nessas duas posições. Esse `-1` é o único caso em que o vetor pode conter um valor fora do intervalo `[0.0, 1.0]`, e serve justamente para distinguir "ausência de dado" de um valor normalizado próximo de zero.


## Constantes de normalização

Alguns valores que aparecem nas fórmulas, como `max_amount` e `max_installments`, estão definidos no arquivo [normalization.json](/resources/normalization.json):

```json
{
  "max_amount": 10000,
  "max_installments": 12,
  "amount_vs_avg_ratio": 10,
  "max_minutes": 1440,
  "max_km": 1000,
  "max_tx_count_24h": 20,
  "max_merchant_avg_amount": 10000
}
```

Para mais detalhes sobre os arquivos de referência (incluindo `mcc_risk.json` e `references.json.gz`), veja [DATASET.md](./DATASET.md).


## Como a decisão é tomada

Depois que o vetor está pronto, a sua API deve:

1. Buscar, no dataset de referência, os 5 vetores mais próximos do vetor da transação que acabou de chegar.
2. Calcular `fraud_score` como a fração de fraudes entre essas 5 referências — ou seja, `número_de_fraudes / 5`.
3. Responder `approved = fraud_score < 0.6`. O threshold de `0.6` é fixo.

Para medir a proximidade dos vetores, os exemplos deste documento usam **distância euclidiana** com *brute force* sobre as 14 dimensões. Note que você é livre pra escolher qualquer algoritmo/técnica de busca vetorial.

> **Importante!** Não é permitido usar os payloads do teste como referência ou para fazer lookup de fraudes! Os testes finais vão usar outros payloads, e fazer isso nas prévias distorce o resultado e desanima outros participantes.


## Exemplo de transação fraudulenta

Para contrastar com o caso legítimo da visão geral, veja como fica uma transação fraudulenta: valor alto, longe de casa, em um comerciante desconhecido, sem histórico de transação anterior. Para o formato completo do payload, veja [API.md](./API.md).

```
1. recebe a requisição:
    {
      "id": "tx-3330991687",
      "transaction":      { "amount": 9505.97, "installments": 10, "requested_at": "2026-03-14T05:15:12Z" },
      "customer":         { "avg_amount": 81.28, "tx_count_24h": 20, "known_merchants": ["MERC-008", "MERC-007", "MERC-005"] },
      "merchant":         { "id": "MERC-068", "mcc": "7802", "avg_amount": 54.86 },
      "terminal":         { "is_online": false, "card_present": true, "km_from_home": 952.27 },
      "last_transaction": null
    }
          ↓
2. vetoriza e normaliza (14 dimensões — note os `-1` nos índices 5 e 6 por conta do `last_transaction: null`):
    [0.9506, 0.8333, 1.0, 0.2174, 0.8333, -1, -1, 0.9523, 1.0, 0, 1, 1, 0.75, 0.0055]
          ↓
3. busca os 5 vizinhos mais próximos:
    dist=0.2315  fraud
    dist=0.2384  fraud
    dist=0.2552  fraud
    dist=0.2667  fraud
    dist=0.2785  fraud
          ↓
4. calcula o score (threshold 0.6):
    score = 5 fraudes / 5 = 1.0
    approved = score < 0.6 → false
          ↓
5. resposta:
    {
      "approved": false,
      "fraud_score": 1.0
    }
```
