#![feature(test)]
#[macro_use]
extern crate diesel;
extern crate dotenv;

use crate::api::*;
use crate::smt::db::cota_db::CotaRocksDB;
use jsonrpc_http_server::jsonrpc_core::serde_json::from_str;
use jsonrpc_http_server::jsonrpc_core::IoHandler;
use jsonrpc_http_server::ServerBuilder;
use lazy_static::lazy_static;
use log::info;
use std::env;

pub mod api;
mod entries;
mod indexer;
mod models;
mod request;
mod response;
pub mod schema;
mod smt;
mod utils;

lazy_static! {
    static ref DB: CotaRocksDB = CotaRocksDB::default();
}

fn main() {
    env_logger::Builder::from_default_env()
        .format_timestamp(Some(env_logger::fmt::TimestampPrecision::Millis))
        .init();
    let mut io = IoHandler::default();
    io.add_method("generate_define_cota_smt", |req| define_rpc(req, &DB));
    io.add_method("generate_mint_cota_smt", |req| mint_rpc(req, &DB));
    io.add_method("generate_withdrawal_cota_smt", |req| {
        withdrawal_rpc(req, &DB)
    });
    io.add_method("generate_claim_cota_smt", |req| claim_rpc(req, &DB));
    io.add_method("generate_update_cota_smt", |req| update_rpc(req, &DB));
    io.add_method("generate_transfer_cota_smt", |req| transfer_rpc(req, &DB));
    io.add_method("generate_claim_update_cota_smt", |req| {
        claim_update_rpc(req, &DB)
    });
    io.add_method("generate_transfer_update_cota_smt", |req| {
        transfer_update_rpc(req, &DB)
    });
    io.add_method("get_hold_cota_nft", fetch_hold_rpc);
    io.add_method("get_withdrawal_cota_nft", fetch_withdrawal_rpc);
    io.add_method("get_mint_cota_nft", fetch_mint_rpc);
    io.add_method("is_claimed", is_claimed_rpc);
    io.add_method("get_cota_nft_sender", get_sender_lock_hash);
    io.add_method("get_define_info", get_define_info);

    let threads: usize = match env::var("THREADS") {
        Ok(thread) => from_str::<usize>(&thread).unwrap(),
        Err(_e) => 3,
    };

    let server = ServerBuilder::new(io)
        .threads(threads)
        .start_http(&"0.0.0.0:3030".parse().unwrap())
        .unwrap();

    let version = env!("CARGO_PKG_VERSION");
    info!("{}", format!("Cota aggregator v{} server start", version));

    server.wait();
}
