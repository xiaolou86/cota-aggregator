use crate::models::withdrawal::{get_withdrawal_cota_by_lock_hash, WithdrawDb};
use crate::request::transfer::{TransferUpdate, TransferUpdateReq};
use crate::smt::common::{
    generate_claim_key, generate_claim_value, generate_history_smt, generate_withdrawal_key,
    generate_withdrawal_key_v1, generate_withdrawal_value, generate_withdrawal_value_v1,
};
use crate::utils::error::Error;
use cota_smt::common::*;
use cota_smt::molecule::prelude::*;
use cota_smt::smt::{blake2b_256, Blake2bHasher, H256};
use cota_smt::transfer_update::TransferUpdateCotaNFTV1EntriesBuilder;
use log::{error, info};

pub fn generate_transfer_update_smt(
    transfer_update_req: TransferUpdateReq,
) -> Result<(String, String), Error> {
    let transfers = transfer_update_req.clone().transfers;
    let transfers_len = transfers.len();
    if transfers_len == 0 {
        return Err(Error::RequestParamNotFound("transfers".to_string()));
    }
    let cota_id_and_token_index_pairs = Some(
        transfers
            .iter()
            .map(|transfer| (transfer.cota_id, transfer.token_index))
            .collect(),
    );
    let sender_withdrawals = get_withdrawal_cota_by_lock_hash(
        transfer_update_req.withdrawal_lock_hash,
        cota_id_and_token_index_pairs,
    )?
    .0;
    if sender_withdrawals.is_empty() || sender_withdrawals.len() != transfers_len {
        return Err(Error::CotaIdAndTokenIndexHasNotWithdrawn);
    }

    let mut action_vec: Vec<u8> = Vec::new();
    if transfers_len == 1 {
        action_vec.extend("Transfer the NFT ".as_bytes());
        action_vec.extend(&sender_withdrawals.first().unwrap().cota_id);
        action_vec.extend(&sender_withdrawals.first().unwrap().token_index);
        action_vec.extend(" to ".as_bytes());
        action_vec.extend(&transfers.first().unwrap().to_lock_script);
        action_vec.extend(" and update the NFT information".as_bytes());
    }
    let action_bytes = BytesBuilder::default()
        .set(action_vec.iter().map(|v| Byte::from(*v)).collect())
        .build();

    let mut claimed_keys: Vec<ClaimCotaNFTKey> = Vec::new();
    let mut claimed_values: Vec<Byte32> = Vec::new();
    let mut claimed_infos: Vec<ClaimCotaNFTInfo> = Vec::new();
    let mut withdrawal_keys: Vec<WithdrawalCotaNFTKeyV1> = Vec::new();
    let mut withdrawal_values: Vec<WithdrawalCotaNFTValueV1> = Vec::new();
    let mut transfer_update_leaves: Vec<(H256, H256)> = Vec::with_capacity(transfers_len * 2);
    let mut withdrawal_update_leaves: Vec<(H256, H256)> = Vec::with_capacity(transfers_len);
    let mut transfer_update_smt =
        generate_history_smt(blake2b_256(&transfer_update_req.lock_script))?;
    let mut withdrawal_smt = generate_history_smt(transfer_update_req.withdrawal_lock_hash)?;
    for (withdrawal_db, transfer) in sender_withdrawals.into_iter().zip(transfers.clone()) {
        let WithdrawDb {
            cota_id,
            token_index,
            characteristic,
            state,
            configure,
            out_point,
            version,
            ..
        } = withdrawal_db;
        let nft_info = CotaNFTInfoBuilder::default()
            .characteristic(Characteristic::from_slice(&characteristic).unwrap())
            .configure(Byte::from(configure))
            .state(Byte::from(state))
            .build();
        let claimed_info = ClaimCotaNFTInfoBuilder::default()
            .nft_info(nft_info)
            .version(Byte::from(1u8))
            .build();
        claimed_infos.push(claimed_info);

        let TransferUpdate { to_lock_script, .. } = transfer;

        let key = if version == 0 {
            generate_withdrawal_key(cota_id, token_index).1
        } else {
            generate_withdrawal_key_v1(cota_id, token_index, out_point).1
        };
        let value = if version == 0 {
            generate_withdrawal_value(
                configure,
                state,
                characteristic,
                transfer_update_req.lock_script.clone(),
                out_point,
            )
            .1
        } else {
            generate_withdrawal_value_v1(configure, state, characteristic, to_lock_script.clone()).1
        };
        withdrawal_update_leaves.push((key, value));
        withdrawal_smt
            .update(key, value)
            .expect("withdraw SMT update leave error");

        let (withdrawal_key, key) = generate_withdrawal_key_v1(
            cota_id,
            token_index,
            transfer_update_req.transfer_out_point,
        );
        let (withdrawal_value, value) = generate_withdrawal_value_v1(
            configure,
            transfer.state,
            transfer.characteristic,
            to_lock_script,
        );
        withdrawal_keys.push(withdrawal_key);
        withdrawal_values.push(withdrawal_value);
        transfer_update_leaves.push((key, value));
        transfer_update_smt
            .update(key, value)
            .expect("transfer update SMT update leave error");

        let (claimed_key, key) = generate_claim_key(cota_id, token_index, out_point);
        claimed_keys.push(claimed_key);

        let (claimed_value, value) = generate_claim_value(version);
        claimed_values.push(claimed_value);
        transfer_update_leaves.push((key, value));
        transfer_update_smt
            .update(key, value)
            .expect("transfer SMT update leave error");
    }

    let root_hash = transfer_update_smt.root().clone();
    let mut root_hash_bytes = [0u8; 32];
    root_hash_bytes.copy_from_slice(root_hash.as_slice());
    let transfer_update_root_hash_hex = hex::encode(root_hash_bytes);

    let transfer_update_merkle_proof = transfer_update_smt
        .merkle_proof(transfer_update_leaves.iter().map(|leave| leave.0).collect())
        .map_err(|e| {
            error!("Transfer update SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Transfer".to_string())
        })?;
    let transfer_update_merkle_proof_compiled = transfer_update_merkle_proof
        .compile(transfer_update_leaves.clone())
        .map_err(|e| {
            error!("Transfer SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Transfer update".to_string())
        })?;

    let transfer_update_merkel_proof_vec: Vec<u8> = transfer_update_merkle_proof_compiled.into();
    let transfer_update_merkel_proof_bytes = BytesBuilder::default()
        .extend(
            transfer_update_merkel_proof_vec
                .iter()
                .map(|v| Byte::from(*v)),
        )
        .build();

    let withdrawal_merkle_proof = withdrawal_smt
        .merkle_proof(
            withdrawal_update_leaves
                .iter()
                .map(|leave| leave.0)
                .collect(),
        )
        .map_err(|e| {
            error!("Transfer SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Transfer update".to_string())
        })?;
    let withdrawal_merkle_proof_compiled = withdrawal_merkle_proof
        .compile(withdrawal_update_leaves.clone())
        .map_err(|e| {
            error!("Transfer update SMT proof error: {:?}", e.to_string());
            Error::SMTProofError("Transfer update".to_string())
        })?;

    let withdrawal_merkel_proof_vec: Vec<u8> = withdrawal_merkle_proof_compiled.into();
    let withdrawal_merkel_proof_bytes = BytesBuilder::default()
        .extend(withdrawal_merkel_proof_vec.iter().map(|v| Byte::from(*v)))
        .build();

    let transfer_update_entries = TransferUpdateCotaNFTV1EntriesBuilder::default()
        .claim_keys(
            ClaimCotaNFTKeyVecBuilder::default()
                .set(claimed_keys)
                .build(),
        )
        .claim_infos(
            ClaimCotaNFTInfoVecBuilder::default()
                .set(claimed_infos)
                .build(),
        )
        .withdrawal_keys(
            WithdrawalCotaNFTKeyV1VecBuilder::default()
                .set(withdrawal_keys)
                .build(),
        )
        .withdrawal_values(
            WithdrawalCotaNFTValueV1VecBuilder::default()
                .set(withdrawal_values)
                .build(),
        )
        .proof(transfer_update_merkel_proof_bytes)
        .withdrawal_proof(withdrawal_merkel_proof_bytes)
        .action(action_bytes)
        .build();

    let transfer_update_entry = hex::encode(transfer_update_entries.as_slice());

    Ok((transfer_update_root_hash_hex, transfer_update_entry))
}
