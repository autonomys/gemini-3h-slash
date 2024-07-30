#![deny(unused_crate_dependencies)]

mod types;

use crate::types::{
    Deposit, DomainEpoch, NominatorStorage, Operator, OperatorNominators, PendingDeposit,
    SharePrice, StorageFundRedeemPrice, Withdrawal, WithdrawalInBalance, WithdrawalInShares,
};
use clap::Parser;
use codec::{Decode, Encode};
use futures::future::join_all;
use sp_core::crypto::{ExposeSecret, SecretString};
use sp_core::sr25519::Pair;
use sp_core::Pair as PairT;
use sp_domains::OperatorId;
use sp_runtime::traits::Zero;
use std::collections::BTreeMap;
use substrate_api_client::ac_compose_macros::log::{debug, error, info};
use substrate_api_client::ac_compose_macros::{compose_call, compose_extrinsic_with_nonce};
use substrate_api_client::ac_primitives::{AssetRuntimeConfig, Config, ExtrinsicSigner};
use substrate_api_client::extrinsic::utility::Batch;
use substrate_api_client::rpc::JsonrpseeClient;
use substrate_api_client::runtime_api::RuntimeApi;
use substrate_api_client::{
    Api as SApi, GetAccountInformation, GetChainInfo, GetStorage, SubmitAndWatch, XtStatus,
};

type Balance = <AssetRuntimeConfig as Config>::Balance;
type Number = <AssetRuntimeConfig as Config>::BlockNumber;
type Hash = <AssetRuntimeConfig as Config>::Hash;
type AccountId = <AssetRuntimeConfig as Config>::AccountId;
type Api = SApi<AssetRuntimeConfig, JsonrpseeClient>;

#[derive(Debug, Parser)]
pub struct Args {
    /// Sudo key
    ///
    /// Example: "//Alice".
    #[arg(long, required = true)]
    keystore_suri: SecretString,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let sudoer = Pair::from_string(args.keystore_suri.expose_secret(), None).unwrap();
    debug!("Sudo public key: {:?}", sudoer.public().to_string());

    let sudoer = ExtrinsicSigner::<AssetRuntimeConfig>::new(sudoer);
    let client = JsonrpseeClient::new("wss://rpc-0.gemini-3h.subspace.network/ws")
        .await
        .unwrap();
    let mut api = SApi::<AssetRuntimeConfig, _>::new(client).await.unwrap();
    api.set_signer(sudoer);

    let slashed_operators = get_slashed_operators(&api).await;
    let fut_storages: Vec<_> = slashed_operators
        .clone()
        .into_iter()
        .map(|slashed_operator| {
            get_nominator_deposits_and_withdrawal(&api, slashed_operator.0, slashed_operator.1)
        })
        .collect();
    let operator_nominators = join_all(fut_storages).await;

    let operator_info_futs = slashed_operators
        .into_iter()
        .map(|(operator_id, block_hash)| get_operator_info(&api, operator_id, block_hash));
    let operators_info = BTreeMap::from_iter(join_all(operator_info_futs).await);

    let futs: Vec<_> = operator_nominators
        .into_iter()
        .map(|operator_nominator| {
            let (operator, block_hash) = operators_info
                .get(&operator_nominator.operator_id)
                .cloned()
                .unwrap();
            calculate_nominators_slashed_amount(
                &api,
                operator_nominator.operator_id,
                operator,
                operator_nominator.nominator_storage,
                block_hash,
            )
        })
        .collect();

    let nominator_slashed_balances = join_all(futs).await;
    let total_balance_slashed =
        nominator_slashed_balances
            .iter()
            .fold(Balance::zero(), |acc, (_, nominator_balances)| {
                acc.checked_add(
                    nominator_balances
                        .iter()
                        .fold(Balance::zero(), |acc, (_, balance)| {
                            acc.checked_add(*balance).unwrap()
                        }),
                )
                .unwrap()
            });
    let treasury_balance = get_treasury_balance(&api).await;
    info!("Treasury Balance: {:?}", treasury_balance);
    info!("Total Slashed: {:?}", total_balance_slashed);
    assert!(
        treasury_balance >= total_balance_slashed,
        "Treasury balance not sufficient for transfer"
    );

    // get the starting nonce of the sudoer and dispatch batch call for each operator
    let mut nonce = api.get_nonce().await.unwrap();
    let futs: Vec<_> = nominator_slashed_balances
        .into_iter()
        .map(|(operator_id, nominator_balances)| {
            let fut = transfer_balance_from_treasury(&api, nonce, operator_id, nominator_balances);
            nonce += 1;
            fut
        })
        .collect();
    join_all(futs).await;
}

async fn get_slashed_operators(api: &Api) -> Vec<(OperatorId, Hash)> {
    let slashed_operators = vec![
        (65, 2364057),
        (41, 2364307),
        (64, 2364389),
        (61, 2364389),
        (30, 2364389),
        (66, 2364761),
        (62, 2364761),
        (78, 2368057),
        (63, 2368101),
        (37, 2368542),
        (77, 2368906),
        (40, 2369910),
        (80, 2374768),
        (81, 2375003),
        (21, 2375130),
        (48, 2375244),
        (71, 2380396),
        (56, 2381733),
        (51, 2383817),
        (6, 2384081),
        (73, 2384081),
        (76, 2384081),
        (10, 2384081),
        (24, 2384516),
        (52, 2386856),
        (79, 2386991),
        (45, 2387166),
        (102, 2388238),
    ];

    let futs: Vec<_> = slashed_operators
        .into_iter()
        .map(|(operator_id, number)| async move {
            (
                operator_id,
                api.get_block_hash(Some(number - 1))
                    .await
                    .ok()
                    .flatten()
                    .unwrap(),
            )
        })
        .collect();
    join_all(futs).await
}

async fn get_nominator_deposits_and_withdrawal(
    api: &Api,
    operator_id: OperatorId,
    block_hash: Hash,
) -> OperatorNominators {
    let deposits = get_nominator_storage::<Deposit>(api, operator_id, block_hash, "Deposits").await;
    let withdrawals =
        get_nominator_storage::<Withdrawal>(api, operator_id, block_hash, "Withdrawals").await;
    let mut storage = BTreeMap::new();
    deposits.into_iter().for_each(|(nominator_id, deposit)| {
        storage.insert(
            nominator_id,
            NominatorStorage {
                deposit,
                withdrawal: None,
            },
        );
    });
    withdrawals
        .into_iter()
        .for_each(|(nominator_id, withdrawal)| {
            match storage.get(&nominator_id) {
                None => panic!(
                    "there will always be a deposit for this nominator even with zero shares"
                ),
                Some(nominator_storage) => storage.insert(
                    nominator_id,
                    NominatorStorage {
                        deposit: nominator_storage.deposit.clone(),
                        withdrawal: Some(withdrawal),
                    },
                ),
            };
        });

    OperatorNominators {
        operator_id,
        nominator_storage: storage,
    }
}

async fn get_nominator_storage<V: Decode>(
    api: &Api,
    operator_id: OperatorId,
    block_hash: Hash,
    storage: &'static str,
) -> Vec<(AccountId, V)> {
    let storage_prefix = api
        .get_storage_double_map_key_prefix("Domains", storage, operator_id)
        .await
        .unwrap();
    let storage_keys = api
        .get_storage_keys_paged(
            Some(storage_prefix.clone()),
            u32::MAX,
            None,
            Some(block_hash),
        )
        .await
        .unwrap();

    let storage_futures: Vec<_> = storage_keys
        .into_iter()
        .map(|storage_key| {
            let api = api.clone();
            let storage_prefix = storage_prefix.clone();

            async move {
                let value = api
                    .get_storage_by_key::<V>(storage_key.clone(), Some(block_hash))
                    .await
                    .ok()
                    .flatten()
                    .unwrap();
                let mut nominator_key = &storage_key.0[storage_prefix.0.len()..];
                let nominator_id = AccountId::decode(&mut nominator_key).unwrap();
                (nominator_id, value)
            }
        })
        .collect();

    join_all(storage_futures).await
}

async fn get_operator_info(
    api: &Api,
    operator_id: OperatorId,
    block_hash: Hash,
) -> (OperatorId, (Operator, Hash)) {
    (
        operator_id,
        (
            api.get_storage_map::<_, Operator>(
                "Domains",
                "Operators",
                operator_id,
                Some(block_hash),
            )
            .await
            .ok()
            .flatten()
            .unwrap(),
            block_hash,
        ),
    )
}

async fn calculate_nominators_slashed_amount(
    api: &Api,
    operator_id: OperatorId,
    mut operator: Operator,
    operator_nominators: BTreeMap<AccountId, NominatorStorage>,
    block_hash: Hash,
) -> (OperatorId, BTreeMap<AccountId, Balance>) {
    let mut total_stake = operator
        .current_total_stake
        .checked_add(operator.current_epoch_rewards)
        .unwrap();

    operator.current_epoch_rewards = Zero::zero();
    let mut total_shares = operator.current_total_shares;
    let share_price = SharePrice::new(total_shares, total_stake);

    let operator_storage_fund_balance =
        get_operator_storage_fund_balance(api, operator_id, block_hash).await;
    let mut total_storage_fee_deposit = operator.total_storage_fee_deposit;

    let mut nominators_slashed_balances = BTreeMap::new();
    let mut nominator_storage_fund_deposited_balances = vec![];
    for (nominator_id, mut nominator_storage) in operator_nominators {
        do_convert_previous_epoch_deposits(
            api,
            operator_id,
            &mut nominator_storage.deposit,
            block_hash,
        )
        .await;

        let (amount_ready_to_withdraw, shares_withdrew_in_current_epoch, storage_fund_withdrew) =
            match nominator_storage.withdrawal {
                None => (Zero::zero(), Zero::zero(), Zero::zero()),
                Some(mut withdrawal) => {
                    do_convert_previous_epoch_withdrawal(
                        api,
                        operator_id,
                        &mut withdrawal,
                        block_hash,
                    )
                    .await;
                    (
                        withdrawal.total_withdrawal_amount,
                        withdrawal
                            .withdrawal_in_shares
                            .map(|WithdrawalInShares { shares, .. }| shares)
                            .unwrap_or_default(),
                        withdrawal.withdrawals.into_iter().fold(
                            Balance::zero(),
                            |acc, withdrawal_in_balance| {
                                acc.checked_add(withdrawal_in_balance.storage_fee_refund)
                                    .unwrap()
                            },
                        ),
                    )
                }
            };

        // deduct any unstaked pending storage fee deposits from the total storage deposits.
        if let Some(pending_deposit) = nominator_storage.deposit.pending {
            total_storage_fee_deposit =
                total_storage_fee_deposit.saturating_sub(pending_deposit.storage_fee_deposit);
        }

        let nominator_shares = nominator_storage
            .deposit
            .known
            .shares
            .checked_add(shares_withdrew_in_current_epoch)
            .unwrap();

        let nominator_staked_amount = share_price.shares_to_stake(nominator_shares);
        total_stake = total_stake.saturating_sub(nominator_staked_amount);
        total_shares = total_shares.saturating_sub(nominator_shares);

        // current staked amount + amount ready to withdraw + withdrawn storage fund
        let total_slashed =
            nominator_staked_amount + amount_ready_to_withdraw + storage_fund_withdrew;
        nominators_slashed_balances.insert(nominator_id.clone(), total_slashed);

        // add remaining storage fund balance that is still in the pool for each nominator
        nominator_storage_fund_deposited_balances.push((
            nominator_id,
            nominator_storage.deposit.known.storage_fee_deposit,
        ))
    }

    // iterate through each nominator storage fund and calculate the actual storage fund based on
    // total storage fund balance.
    nominator_storage_fund_deposited_balances
        .into_iter()
        .for_each(|(nominator_id, deposited_balance)| {
            let storage_fund_share_price = StorageFundRedeemPrice::new(
                operator_storage_fund_balance,
                total_storage_fee_deposit,
            );
            let storage_fund_slashed = storage_fund_share_price.redeem(deposited_balance);
            let existing_balance = nominators_slashed_balances
                .get(&nominator_id)
                .cloned()
                .unwrap();
            nominators_slashed_balances
                .insert(nominator_id, existing_balance + storage_fund_slashed);
        });

    (operator_id, nominators_slashed_balances)
}

async fn do_convert_previous_epoch_deposits(
    api: &Api,
    operator_id: OperatorId,
    deposit: &mut Deposit,
    block_hash: Hash,
) {
    // if it is one of the previous domain epoch, then calculate shares for the epoch and update known deposit
    let pending_deposit = match deposit.pending.take() {
        None => return,
        Some(pd) => pd,
    };

    let PendingDeposit {
        effective_domain_epoch,
        amount,
        storage_fee_deposit,
    } = pending_deposit;

    if let Some(epoch_share_price) =
        get_operator_epoch_share_price(api, operator_id, effective_domain_epoch, block_hash).await
    {
        let new_shares = epoch_share_price.stake_to_shares(amount);
        deposit.known.shares = deposit.known.shares.checked_add(new_shares).unwrap();
        deposit.known.storage_fee_deposit = deposit
            .known
            .storage_fee_deposit
            .checked_add(storage_fee_deposit)
            .unwrap();
    }
}

async fn do_convert_previous_epoch_withdrawal(
    api: &Api,
    operator_id: OperatorId,
    withdrawal: &mut Withdrawal,
    block_hash: Hash,
) {
    let pending_withdrawal = match withdrawal.withdrawal_in_shares.take() {
        None => return,
        Some(pw) => pw,
    };

    let WithdrawalInShares {
        domain_epoch,
        unlock_at_confirmed_domain_block_number,
        shares,
        storage_fee_refund,
    } = pending_withdrawal;

    if let Some(epoch_share_price) =
        get_operator_epoch_share_price(api, operator_id, domain_epoch, block_hash).await
    {
        let withdrawal_amount = epoch_share_price.shares_to_stake(shares);

        withdrawal.total_withdrawal_amount = withdrawal
            .total_withdrawal_amount
            .checked_add(withdrawal_amount)
            .unwrap();

        let (domain_id, _) = domain_epoch.deconstruct();

        let withdraw_in_balance = WithdrawalInBalance {
            domain_id,
            unlock_at_confirmed_domain_block_number,
            amount_to_unlock: withdrawal_amount,
            storage_fee_refund,
        };
        withdrawal.withdrawals.push_back(withdraw_in_balance);
    }
}

async fn get_operator_epoch_share_price(
    api: &Api,
    operator_id: OperatorId,
    domain_epoch: DomainEpoch,
    block_hash: Hash,
) -> Option<SharePrice> {
    api.get_storage_double_map::<_, _, SharePrice>(
        "Domains",
        "OperatorEpochSharePrice",
        operator_id,
        domain_epoch,
        Some(block_hash),
    )
    .await
    .ok()
    .flatten()
}

async fn get_operator_storage_fund_balance(
    api: &Api,
    operator_id: OperatorId,
    block_hash: Hash,
) -> Balance {
    let runtime_api = api.runtime_api();
    runtime_api
        .runtime_call::<Balance>(
            "DomainsApi_storage_fund_account_balance",
            vec![operator_id.encode()],
            Some(block_hash),
        )
        .await
        .unwrap()
}

async fn get_treasury_balance(api: &Api) -> Balance {
    let treasury_account = api
        .get_constant::<AccountId>("Domains", "TreasuryAccount")
        .await
        .unwrap();
    api.get_account_data(&treasury_account)
        .await
        .ok()
        .flatten()
        .unwrap()
        .free
}

async fn transfer_balance_from_treasury(
    api: &Api,
    nonce: u32,
    operator_id: OperatorId,
    nominator_balances: BTreeMap<AccountId, Balance>,
) {
    debug!("Sending batch transfer for Operator[{operator_id:?}] with Nonce[{nonce}] for {:?} Nominators", nominator_balances.len());
    let metadata = api.metadata();
    let transfer_calls = nominator_balances
        .into_iter()
        .map(|(acc, balance)| {
            compose_call!(metadata, "Domains", "transfer_treasury_funds", acc, balance).unwrap()
        })
        .collect();

    let calls = Batch {
        calls: transfer_calls,
    };
    let batch_call = compose_call!(metadata, "Utility", "batch_all", calls).unwrap();
    let xt = compose_extrinsic_with_nonce!(&api, nonce, "Sudo", "sudo", batch_call).unwrap();
    let result = api
        .submit_and_watch_extrinsic_until(xt, XtStatus::InBlock)
        .await;
    match result {
        Ok(res) => {
            info!(
                "Batch extrinsic for Operator[{operator_id:?}] included in block: {:?}",
                res.block_hash
            );
        }
        Err(err) => {
            error!(
                "Failed to submit batch for Operator[{operator_id:?}]: {:?}",
                err
            )
        }
    };
}
