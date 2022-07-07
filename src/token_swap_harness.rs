use std::{borrow::Borrow, sync::Arc};

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};

use spl_token_swap;

use crate::token_helpers;

const ZERO_FEE: spl_token_swap::curve::fees::Fees = spl_token_swap::curve::fees::Fees {
    trade_fee_numerator: 0,
    trade_fee_denominator: 1,
    owner_trade_fee_numerator: 0,
    owner_trade_fee_denominator: 1,
    owner_withdraw_fee_numerator: 0,
    owner_withdraw_fee_denominator: 1,
    host_fee_numerator: 0,
    host_fee_denominator: 1,
};

pub struct TokenSwapPoolHarness {
    pool_key: Pubkey,
}

fn find_pool_authority_address(pool_pubkey: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[pool_pubkey.as_ref()], &spl_token_swap::id()).0
}

/// A harness to setup and create transactions for a spl-token-swap pools
impl TokenSwapPoolHarness {
    pub fn create_swap_instruction(
        &self,
        user: &Pubkey,
        user_transfer_authority_address: &Pubkey,
        a_to_b: bool,
        swap: spl_token_swap::instruction::Swap,
        rpc_client: &RpcClient,
    ) -> Instruction {
        let pool_account = rpc_client.get_account(&self.pool_key).unwrap();
        let token_swap =
            spl_token_swap::state::SwapVersion::unpack(&pool_account.data.borrow()).unwrap();

        let pool_authority_address = find_pool_authority_address(&self.pool_key);
        let user_token_a_account_address =
            spl_associated_token_account::get_associated_token_address(
                user,
                token_swap.token_a_mint(),
            );
        let user_token_b_account_address =
            spl_associated_token_account::get_associated_token_address(
                user,
                token_swap.token_b_mint(),
            );

        let (source_pubkey, destination_pubkey) = if a_to_b {
            (user_token_a_account_address, user_token_b_account_address)
        } else {
            (user_token_b_account_address, user_token_a_account_address)
        };

        let (swap_source_pubkey, swap_destination_pubkey) = if a_to_b {
            (token_swap.token_a_account(), token_swap.token_b_account())
        } else {
            (token_swap.token_b_account(), token_swap.token_a_account())
        };

        spl_token_swap::instruction::swap(
            &spl_token_swap::id(),
            &spl_token::id(),
            &&self.pool_key,
            &pool_authority_address,
            user_transfer_authority_address,
            &source_pubkey,
            &swap_source_pubkey,
            &swap_destination_pubkey,
            &destination_pubkey,
            &token_swap.pool_mint(),
            &token_swap.pool_fee_account(),
            None,
            swap,
        )
        .unwrap()
    }

    pub fn get_keys(&self, rpc_client: &RpcClient) -> Vec<Pubkey> {
        let pool_account = rpc_client.get_account(&self.pool_key).unwrap();
        let token_swap =
            spl_token_swap::state::SwapVersion::unpack(&pool_account.data.borrow()).unwrap();

        vec![
            self.pool_key,
            find_pool_authority_address(&self.pool_key),
            *token_swap.token_a_account(),
            *token_swap.token_b_account(),
            *token_swap.pool_mint(),
            *token_swap.pool_fee_account(),
        ]
    }
}

pub fn initialize_pool(
    payer: &Keypair,
    token_a_mint: &Pubkey,
    token_b_mint: &Pubkey,
    token_a_initial_liquidity: u64,
    token_b_initial_liquidity: u64,
    rpc_client: &RpcClient,
) -> TokenSwapPoolHarness {
    let pool_keypair = Keypair::new();

    let pool_authority_address = find_pool_authority_address(&pool_keypair.pubkey());

    let mut setup_ixs = Vec::new();

    // Create atas
    let (token_a, ix) = token_helpers::create_ata(payer, token_a_mint, &pool_authority_address);
    setup_ixs.push(ix);

    setup_ixs.push(
        spl_token::instruction::mint_to(
            &spl_token::id(),
            token_a_mint,
            &token_a,
            &payer.pubkey(),
            &[],
            token_a_initial_liquidity,
        )
        .unwrap(),
    );

    let (token_b, ix) = token_helpers::create_ata(payer, token_b_mint, &pool_authority_address);
    setup_ixs.push(ix);

    setup_ixs.push(
        spl_token::instruction::mint_to(
            &spl_token::id(),
            token_b_mint,
            &token_b,
            &payer.pubkey(),
            &[],
            token_b_initial_liquidity,
        )
        .unwrap(),
    );

    // create Lp token
    let pool_lp_keypair = Keypair::new();
    let ixs = token_helpers::initialize_mint(
        &payer,
        &pool_lp_keypair,
        &pool_authority_address,
        6,
        rpc_client,
    );
    setup_ixs.extend(ixs);

    let (fee_pubkey, ix) = token_helpers::create_ata(
        payer,
        &pool_lp_keypair.pubkey(),
        &Keypair::new().pubkey(), // Random owner
    );
    setup_ixs.push(ix);

    let (destination_pubkey, ix) =
        token_helpers::create_ata(payer, &pool_lp_keypair.pubkey(), &Keypair::new().pubkey());
    setup_ixs.push(ix);

    // Send the setup ixs
    let latest_blockhash = rpc_client.get_latest_blockhash().unwrap();
    rpc_client
        .send_and_confirm_transaction(&Transaction::new_signed_with_payer(
            &setup_ixs,
            Some(&payer.pubkey()),
            &[payer, &pool_lp_keypair],
            latest_blockhash,
        ))
        .unwrap();

    let swap_info_account_rent = rpc_client
        .get_minimum_balance_for_rent_exemption(spl_token_swap::state::SwapVersion::LATEST_LEN)
        .unwrap();

    let latest_blockhash = rpc_client.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),
                &pool_keypair.pubkey(),
                swap_info_account_rent,
                spl_token_swap::state::SwapVersion::LATEST_LEN as u64,
                &spl_token_swap::id(),
            ),
            spl_token_swap::instruction::initialize(
                &spl_token_swap::id(),
                &spl_token::id(),
                &pool_keypair.pubkey(),
                &pool_authority_address,
                &token_a,
                &token_b,
                &pool_lp_keypair.pubkey(),
                &fee_pubkey,
                &destination_pubkey,
                ZERO_FEE,
                spl_token_swap::curve::base::SwapCurve {
                    curve_type: spl_token_swap::curve::base::CurveType::ConstantProduct,
                    calculator: Arc::new(
                        spl_token_swap::curve::constant_product::ConstantProductCurve,
                    ),
                },
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
        &[payer, &pool_keypair],
        latest_blockhash,
    );

    let signature = rpc_client.send_and_confirm_transaction(&tx).unwrap();
    println!("init pool: {signature}");

    TokenSwapPoolHarness {
        pool_key: pool_keypair.pubkey(),
    }
}
