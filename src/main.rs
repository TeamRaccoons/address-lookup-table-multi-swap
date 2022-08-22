use std::collections::HashSet;
use std::fs;
use std::str::FromStr;
use std::thread;
use std::time;

use anyhow::Result;
use bincode::serialize;
use reqwest;
use serde_json::json;
use solana_address_lookup_table_program::{self, state::AddressLookupTable};
use solana_client::{
    rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig, rpc_request::RpcRequest,
};
use solana_sdk::commitment_config::CommitmentLevel;
use solana_sdk::{
    self,
    address_lookup_table_account::AddressLookupTableAccount,
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature},
    signer::Signer,
    transaction::{Transaction, VersionedTransaction},
};
use solana_transaction_status::UiTransactionEncoding;

const NUMBER_OF_MINTS: usize = 26;

use alt_demo::{token_helpers, token_swap_harness};

pub fn main() {
    let payer = read_keypair_file(&*shellexpand::tilde("~/.config/solana/id.json")).unwrap();
    let rpc_client =
        RpcClient::new_with_commitment("http://localhost:8899", CommitmentConfig::confirmed());

    println!("Create {NUMBER_OF_MINTS} mints and the corresponding ata for the payer");
    let mut token_mints = Vec::new();

    for _ in 0..NUMBER_OF_MINTS {
        let token_mint_keypair = Keypair::new();

        let mut ixs = token_helpers::initialize_mint(
            &payer,
            &token_mint_keypair,
            &payer.pubkey(),
            6,
            &rpc_client,
        );

        let (ata, create_ata_ix) =
            token_helpers::create_ata(&payer, &token_mint_keypair.pubkey(), &payer.pubkey());
        ixs.push(create_ata_ix);

        let latest_blockhash = rpc_client.get_latest_blockhash().unwrap();
        rpc_client
            .send_and_confirm_transaction(&Transaction::new_signed_with_payer(
                &ixs,
                Some(&payer.pubkey()),
                &[&payer, &token_mint_keypair],
                latest_blockhash,
            ))
            .unwrap();

        token_mints.push(token_mint_keypair.pubkey());
    }

    println!(
        "Create {} pools, mint0/mint1 => ... => mintN-1/mintN",
        NUMBER_OF_MINTS - 1
    );
    let mut swap_ixs = Vec::new();
    for i in 0..(NUMBER_OF_MINTS - 1) {
        let token_swap_harness = token_swap_harness::initialize_pool(
            &payer,
            &token_mints[i],
            &token_mints[i + 1],
            1_000_000,
            1_000_000,
            &rpc_client,
        );

        let ix = token_swap_harness.create_swap_instruction(
            &payer.pubkey(),
            &payer.pubkey(),
            true,
            spl_token_swap::instruction::Swap {
                amount_in: 1_000 - i as u64 * 10, // Slow decay to account for cpamm formula
                minimum_amount_out: 0,
            },
            &rpc_client,
        );
        swap_ixs.push(ix);
    }

    // The keys we can store in address lookup tables to reduce transaction size
    let mut keys = HashSet::new();
    swap_ixs.iter().for_each(|ix| {
        ix.accounts.iter().for_each(|account| {
            keys.insert(account.pubkey);
        })
    });

    println!("mint some mint0 tokens to swap all the way to mintN");
    let latest_blockhash = rpc_client.get_latest_blockhash().unwrap();
    let ata = spl_associated_token_account::get_associated_token_address(
        &payer.pubkey(),
        &token_mints[0],
    );
    rpc_client
        .send_and_confirm_transaction(&Transaction::new_signed_with_payer(
            &[spl_token::instruction::mint_to(
                &spl_token::id(),
                &token_mints[0],
                &ata,
                &payer.pubkey(),
                &[],
                1_000,
            )
            .unwrap()],
            Some(&payer.pubkey()),
            &[&payer],
            latest_blockhash,
        ))
        .unwrap();

    println!(
        "Create account lookup table and put all {} keys inside it",
        keys.len()
    );
    let recent_slot = rpc_client
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .unwrap();
    let (create_ix, table_pk) =
        solana_address_lookup_table_program::instruction::create_lookup_table(
            payer.pubkey(),
            payer.pubkey(),
            recent_slot,
        );
    println!("address lookup table pk: {}", table_pk);

    let latest_blockhash = rpc_client.get_latest_blockhash().unwrap();
    rpc_client
        .send_and_confirm_transaction(&Transaction::new_signed_with_payer(
            &[create_ix],
            Some(&payer.pubkey()),
            &[&payer],
            latest_blockhash,
        ))
        .unwrap();

    println!("Loop to extend the address lookup table");
    let mut signature = Signature::default();
    let latest_blockhash = rpc_client.get_latest_blockhash().unwrap();
    for selected_pool_keys in keys.into_iter().collect::<Vec<Pubkey>>().chunks(20) {
        let extend_ix = solana_address_lookup_table_program::instruction::extend_lookup_table(
            table_pk,
            payer.pubkey(),
            Some(payer.pubkey()),
            selected_pool_keys.to_vec(),
        );

        signature = rpc_client
            .send_and_confirm_transaction(&Transaction::new_signed_with_payer(
                &[extend_ix],
                Some(&payer.pubkey()),
                &[&payer],
                latest_blockhash,
            ))
            .unwrap();
    }
    rpc_client
        .confirm_transaction_with_spinner(
            &signature,
            &latest_blockhash,
            CommitmentConfig::finalized(),
        )
        .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &swap_ixs,
        Some(&payer.pubkey()),
        &[&payer],
        latest_blockhash,
    );
    let serialized_tx = serialize(&tx).unwrap();

    println!("This legacy serialized tx is {} bytes", serialized_tx.len());

    println!("Wait some arbitrary amount of time to please the address lookup table");
    thread::sleep(time::Duration::from_secs(5));

    println!("Create multi hop swap going through each pools and show txid");
    let versioned_tx =
        create_tx_with_address_lookup_table(&rpc_client, &swap_ixs, table_pk, &payer).unwrap();
    let serialized_versioned_tx = serialize(&versioned_tx).unwrap();
    println!(
        "The serialized versioned tx is {} bytes",
        serialized_versioned_tx.len()
    );
    let serialized_encoded = base64::encode(serialized_versioned_tx);
    let config = RpcSendTransactionConfig {
        skip_preflight: false,
        preflight_commitment: Some(CommitmentLevel::Processed),
        encoding: Some(UiTransactionEncoding::Base64),
        ..RpcSendTransactionConfig::default()
    };

    let signature = rpc_client
        .send::<String>(
            RpcRequest::SendTransaction,
            json!([serialized_encoded, config]),
        )
        .unwrap();
    println!("Multi swap txid: {}", signature);
    rpc_client
        .confirm_transaction_with_commitment(
            &Signature::from_str(signature.as_str()).unwrap(),
            CommitmentConfig::finalized(),
        )
        .unwrap();

    thread::sleep(time::Duration::from_secs(2)); // Not sure why this is required while commitments are compatible

    // We craft our own getTransaction as RpcClient doesn't support v0
    let client = reqwest::blocking::Client::new();
    let res = client
        .post("http://localhost:8899/")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": [
                signature,
                {"encoding": "json", "commitment": "confirmed", "maxSupportedTransactionVersion": 0}
            ]
        }))
        .send()
        .unwrap();
    let response_json: serde_json::Value = res.json().unwrap();
    println!("{:?}", response_json);

    fs::write(
        "response.json",
        serde_json::to_string_pretty(&response_json).unwrap(),
    )
    .unwrap();
}

// from https://github.com/solana-labs/solana/blob/10d677a0927b2ca450b784f750477f05ff6afffe/sdk/program/src/message/versions/v0/mod.rs#L209
fn create_tx_with_address_lookup_table(
    client: &RpcClient,
    instructions: &[Instruction],
    address_lookup_table_key: Pubkey,
    payer: &Keypair,
) -> Result<VersionedTransaction> {
    let raw_account = client.get_account(&address_lookup_table_key)?;
    let address_lookup_table = AddressLookupTable::deserialize(&raw_account.data)?;
    let address_lookup_table_account = AddressLookupTableAccount {
        key: address_lookup_table_key,
        addresses: address_lookup_table.addresses.to_vec(),
    };

    let blockhash = client.get_latest_blockhash()?;
    let tx = VersionedTransaction::try_new(
        VersionedMessage::V0(v0::Message::try_compile(
            &payer.pubkey(),
            instructions,
            &[address_lookup_table_account],
            blockhash,
        )?),
        &[payer],
    )?;

    assert!(tx.message.address_table_lookups().unwrap().len() > 0);
    Ok(tx)
}
