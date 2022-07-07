# Address lookup table demo

Showcases 20 swap chain on spl-token-swap

`solana-test-validator --bpf-program SwapsVeCiPHMUAtzQWZw7RjsKjgCjhwU55QGu4U1Szw bins/spl_token_swap.so --reset`

The was dumped from devnet, there is no new deployment of spl token swap on mainnet-beta

`solana program dump SwapsVeCiPHMUAtzQWZw7RjsKjgCjhwU55QGu4U1Szw spl_token_swap.so -ud`

curl http://localhost:8899 -X POST -H "Content-Type: application/json" -d '
{
"jsonrpc": "2.0",
"id": 1,
"method": "getTransaction",
"params": [
"5VDpUWCdyge3i8ukEfyNukGQdy89fm5B9NrHAuUjKy5zhGdh4cXvSURdgrTuorXqTUYNYUCebZmXtxAnbeeGt1Wf",
{"encoding": "json", "maxSupportedTransactionVersion":0}
]
}
'
