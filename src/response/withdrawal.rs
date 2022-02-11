use crate::models::withdrawal::WithdrawNFTDb;
use jsonrpc_http_server::jsonrpc_core::serde_json::{Map, Number};
use jsonrpc_http_server::jsonrpc_core::Value;

pub fn parse_withdrawal_response(
    withdrawals: Vec<WithdrawNFTDb>,
    total: i64,
    page_size: i64,
) -> Map<String, Value> {
    let nfts: Vec<Value> = withdrawals
        .into_iter()
        .map(parse_withdrawal_value)
        .collect();
    let mut map = Map::new();
    map.insert("total".to_string(), Value::Number(Number::from(total)));
    map.insert(
        "page_size".to_string(),
        Value::Number(Number::from(page_size)),
    );
    map.insert("nfts".to_string(), Value::Array(nfts));
    map
}

fn parse_withdrawal_value(withdrawal: WithdrawNFTDb) -> Value {
    let mut map = Map::new();
    map.insert(
        "cota_id".to_string(),
        Value::String(format!("0x{}", hex::encode(&withdrawal.cota_id))),
    );
    map.insert(
        "index".to_string(),
        Value::String(format!("0x{}", hex::encode(&withdrawal.token_index))),
    );
    map.insert(
        "state".to_string(),
        Value::String(format!("0x{}", hex::encode(&[withdrawal.state]))),
    );
    map.insert(
        "configure".to_string(),
        Value::String(format!("0x{}", hex::encode(&[withdrawal.configure]))),
    );
    map.insert(
        "characteristic".to_string(),
        Value::String(format!("0x{}", hex::encode(&withdrawal.characteristic))),
    );
    Value::Object(map)
}