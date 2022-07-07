use solana_client::{rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use spl_associated_token_account;
use spl_token;

pub fn initialize_mint(
    payer: &Keypair,
    token_mint: &Keypair,
    authority: &Pubkey,
    decimals: u8,
    rpc_client: &RpcClient,
) -> Vec<Instruction> {
    let rent_exempt_threshold = rpc_client
        .get_minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN)
        .unwrap();
    vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &token_mint.pubkey(),
            rent_exempt_threshold,
            spl_token::state::Mint::LEN as u64,
            &spl_token::id(),
        ),
        spl_token::instruction::initialize_mint(
            &spl_token::id(),
            &token_mint.pubkey(),
            authority,
            None,
            decimals,
        )
        .unwrap(),
    ]
}

pub fn create_ata(
    payer: &Keypair,
    token_mint: &Pubkey,
    authority: &Pubkey,
) -> (Pubkey, Instruction) {
    let ata = spl_associated_token_account::get_associated_token_address(authority, token_mint);
    let ix = spl_associated_token_account::instruction::create_associated_token_account(
        &payer.pubkey(),
        &authority,
        token_mint,
    );
    (ata, ix)
}

pub fn create_token_account(
    payer: &Keypair,
    token_mint: &Pubkey,
    authority: &Pubkey,
    rpc_client: &RpcClient,
) -> (Keypair, Vec<Instruction>) {
    let token_account_keypair = Keypair::new();

    let rent_exempt_threshold = rpc_client
        .get_minimum_balance_for_rent_exemption(spl_token::state::Account::LEN)
        .unwrap();
    let ixs = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &token_account_keypair.pubkey(),
            rent_exempt_threshold,
            spl_token::state::Account::LEN as u64,
            &spl_token::id(),
        ),
        spl_token::instruction::initialize_account(
            &spl_token::id(),
            &token_account_keypair.pubkey(),
            &token_mint,
            authority,
        )
        .unwrap(),
    ];
    (token_account_keypair, ixs)
}

pub fn get_token_account(
    token_account_address: &Pubkey,
    rpc_client: &RpcClient,
) -> spl_token::state::Account {
    let account = rpc_client.get_account(token_account_address).unwrap();

    spl_token::state::Account::unpack_from_slice(&account.data).unwrap()
}
