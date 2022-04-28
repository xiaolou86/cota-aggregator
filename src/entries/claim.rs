use crate::entries::helper::{
    generate_claim_key, generate_claim_value, generate_hold_key, generate_hold_value,
    generate_withdrawal_key, generate_withdrawal_key_v1, generate_withdrawal_value,
    generate_withdrawal_value_v1,
};
use crate::entries::smt::{generate_history_smt, init_smt};
use crate::entries::SMT_LOCK;
use crate::indexer::index::get_cota_smt_root;
use crate::models::withdrawal::{get_withdrawal_cota_by_lock_hash, WithdrawDb};
use crate::request::claim::ClaimReq;
use crate::smt::db::db::RocksDB;
use crate::smt::transaction::store_transaction::StoreTransaction;
use crate::smt::RootSaver;
use crate::utils::error::Error;
use cota_smt::common::*;
use cota_smt::molecule::prelude::*;
use cota_smt::smt::{blake2b_256, H256};
use cota_smt::transfer::{ClaimCotaNFTEntries, ClaimCotaNFTEntriesBuilder};
use log::error;
use std::sync::Arc;

pub async fn generate_claim_smt(
    db: &RocksDB,
    claim_req: ClaimReq,
) -> Result<(H256, ClaimCotaNFTEntries), Error> {
    let claims = claim_req.claims;
    let claims_len = claims.len();
    if claims_len == 0 {
        return Err(Error::RequestParamNotFound("claims".to_string()));
    }
    let cota_id_and_token_index_pairs = Some(
        claims
            .iter()
            .map(|claim| (claim.cota_id, claim.token_index))
            .collect(),
    );
    let sender_withdrawals = get_withdrawal_cota_by_lock_hash(
        blake2b_256(claim_req.withdrawal_lock_script.as_slice()),
        cota_id_and_token_index_pairs,
    )?
    .0;
    if sender_withdrawals.is_empty() || sender_withdrawals.len() != claims_len {
        return Err(Error::CotaIdAndTokenIndexHasNotWithdrawn);
    }

    let mut hold_keys: Vec<CotaNFTId> = Vec::new();
    let mut hold_values: Vec<CotaNFTInfo> = Vec::new();
    let mut withdrawal_update_leaves: Vec<(H256, H256)> = Vec::with_capacity(claims_len);
    let mut claim_keys: Vec<ClaimCotaNFTKey> = Vec::new();
    let mut key_vec: Vec<(H256, u8)> = Vec::new();
    let mut claim_values: Vec<Byte32> = Vec::new();
    let mut claim_update_leaves: Vec<(H256, H256)> = Vec::with_capacity(claims_len * 2);
    let mut previous_leaves: Vec<(H256, H256)> = Vec::with_capacity(claims_len * 2);
    for withdrawal in sender_withdrawals {
        let WithdrawDb {
            cota_id,
            token_index,
            characteristic,
            state,
            configure,
            out_point,
            version,
            ..
        } = withdrawal;
        let (key, value) = if version == 0 {
            (
                generate_withdrawal_key(cota_id, token_index).1,
                generate_withdrawal_value(
                    configure,
                    state,
                    characteristic,
                    &claim_req.lock_script,
                    out_point,
                )
                .1,
            )
        } else {
            (
                generate_withdrawal_key_v1(cota_id, token_index, out_point).1,
                generate_withdrawal_value_v1(
                    configure,
                    state,
                    characteristic,
                    &claim_req.lock_script,
                )
                .1,
            )
        };
        withdrawal_update_leaves.push((key, value));
        previous_leaves.push((key, H256::zero()));

        let (hold_key, key) = generate_hold_key(cota_id, token_index);
        let (hold_value, value) = generate_hold_value(configure, state, characteristic);
        hold_keys.push(hold_key);
        hold_values.push(hold_value);
        claim_update_leaves.push((key, value));
        previous_leaves.push((key, H256::zero()));

        let (claim_key, key) = generate_claim_key(cota_id, token_index, out_point);
        claim_keys.push(claim_key);
        key_vec.push((key, version));
    }

    for (key, version) in key_vec {
        let (claim_value, value) = generate_claim_value(version);
        claim_values.push(claim_value);
        claim_update_leaves.push((key, value))
    }

    let claim_smt_root = get_cota_smt_root(&claim_req.lock_script).await?;

    let transaction = &StoreTransaction::new(db.transaction());
    let mut claim_smt = init_smt(transaction, claim_req.lock_script.as_slice())?;
    // Add lock to claim smt
    let &(ref claim_lock, ref claim_cond) = &*Arc::clone(&SMT_LOCK);
    let claim_no_pending = {
        let mut set = claim_lock.lock();
        set.insert(claim_req.lock_script.clone())
    };
    loop {
        if claim_no_pending {
            claim_smt = generate_history_smt(claim_smt, &claim_req.lock_script, claim_smt_root)?;
            claim_smt
                .update_all(claim_update_leaves.clone())
                .expect("claim SMT update leave error");
            claim_smt.save_root_and_leaves(previous_leaves)?;
            claim_smt.commit()?;
            {
                let mut set = claim_lock.lock();
                set.remove(&claim_req.lock_script);
            }
            claim_cond.notify_all();
            break;
        } else {
            let mut set = claim_lock.lock();
            claim_cond.wait(&mut set);
        }
    }

    let withdrawal_smt_root =
        get_cota_smt_root(claim_req.withdrawal_lock_script.as_slice()).await?;
    let transaction = &StoreTransaction::new(db.transaction());
    let mut withdrawal_smt = init_smt(transaction, claim_req.withdrawal_lock_script.as_slice())?;
    // Add lock to withdraw smt
    let &(ref withdraw_lock, ref withdraw_cond) = &*Arc::clone(&SMT_LOCK);
    let withdraw_no_pending = {
        let mut set = withdraw_lock.lock();
        set.insert(claim_req.withdrawal_lock_script.clone())
    };
    loop {
        if withdraw_no_pending {
            withdrawal_smt = generate_history_smt(
                withdrawal_smt,
                claim_req.withdrawal_lock_script.as_slice(),
                withdrawal_smt_root,
            )?;
            withdrawal_smt.save_root_and_leaves(vec![])?;
            withdrawal_smt.commit()?;
            {
                let mut set = withdraw_lock.lock();
                set.remove(&claim_req.withdrawal_lock_script);
            }
            withdraw_cond.notify_all();
            break;
        } else {
            let mut set = withdraw_lock.lock();
            withdraw_cond.wait(&mut set);
        }
    }

    let claim_merkle_proof = claim_smt
        .merkle_proof(claim_update_leaves.iter().map(|leave| leave.0).collect())
        .map_err(|e| {
            error!("Claim SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Claim".to_string())
        })?;
    let claim_merkle_proof_compiled = claim_merkle_proof
        .compile(claim_update_leaves.clone())
        .map_err(|e| {
            error!("Claim SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Claim".to_string())
        })?;

    let merkel_proof_vec: Vec<u8> = claim_merkle_proof_compiled.into();
    let claim_proof = BytesBuilder::default()
        .extend(merkel_proof_vec.iter().map(|v| Byte::from(*v)))
        .build();

    let withdraw_merkle_proof = withdrawal_smt
        .merkle_proof(
            withdrawal_update_leaves
                .iter()
                .map(|leave| leave.0)
                .collect(),
        )
        .map_err(|e| {
            error!("Withdraw SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Withdraw".to_string())
        })?;
    let withdraw_merkle_proof_compiled = withdraw_merkle_proof
        .compile(withdrawal_update_leaves.clone())
        .map_err(|e| {
            error!("Withdraw SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Withdraw".to_string())
        })?;

    let merkel_proof_vec: Vec<u8> = withdraw_merkle_proof_compiled.into();
    let withdrawal_proof = BytesBuilder::default()
        .extend(merkel_proof_vec.iter().map(|v| Byte::from(*v)))
        .build();

    let mut action_vec: Vec<u8> = Vec::new();
    action_vec.extend("Claim ".as_bytes());
    action_vec.extend(&[0u8, 0, 0, claims_len as u8]);
    action_vec.extend(" NFTs".as_bytes());
    let action_bytes = BytesBuilder::default()
        .set(action_vec.iter().map(|v| Byte::from(*v)).collect())
        .build();

    let claim_entries = ClaimCotaNFTEntriesBuilder::default()
        .hold_keys(HoldCotaNFTKeyVecBuilder::default().set(hold_keys).build())
        .hold_values(
            HoldCotaNFTValueVecBuilder::default()
                .set(hold_values)
                .build(),
        )
        .claim_keys(ClaimCotaNFTKeyVecBuilder::default().set(claim_keys).build())
        .claim_values(
            ClaimCotaNFTValueVecBuilder::default()
                .set(claim_values)
                .build(),
        )
        .proof(claim_proof)
        .withdrawal_proof(withdrawal_proof)
        .action(action_bytes)
        .build();

    Ok((*claim_smt.root(), claim_entries))
}
