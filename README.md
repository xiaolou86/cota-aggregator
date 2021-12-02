# compact-nft-aggregator

The aggregator of compact-NFT service

### Quick Start
```shell
cargo build

cargo run

cargo test
```

### Usage

```shell
echo '{
    "id": 2,
    "jsonrpc": "2.0",
    "method": "generate_mint_compact_nft",
    "params": ["0xea28c98f38b4a57aa81756b167bb37fa42daf67edbc9863afb8172096ed301c2"]
}' \
| tr -d '\n' \
| curl -H 'content-type: application/json' -d @- \
http://localhost:3030
```