mod types;

use crate::types::{Deposit, NominatorStorage, OperatorNominators, Withdrawal};
use codec::Decode;
use futures::future::join_all;
use sp_domains::OperatorId;
use std::collections::BTreeMap;
use substrate_api_client::ac_primitives::{AssetRuntimeConfig, Config};
use substrate_api_client::rpc::JsonrpseeClient;
use substrate_api_client::{Api as SApi, GetChainInfo, GetStorage};

type Balance = <AssetRuntimeConfig as Config>::Balance;
type Number = <AssetRuntimeConfig as Config>::BlockNumber;
type AccountId = <AssetRuntimeConfig as Config>::AccountId;
type Api = SApi<AssetRuntimeConfig, JsonrpseeClient>;

#[tokio::main]
async fn main() {
    let client = JsonrpseeClient::new("wss://rpc-0.gemini-3h.subspace.network/ws")
        .await
        .unwrap();
    let api = SApi::<AssetRuntimeConfig, _>::new(client).await.unwrap();
    let slashed_operators = get_slashed_operators();
    let fut_storages: Vec<_> = slashed_operators
        .into_iter()
        .map(|slashed_operator| {
            get_nominator_deposits_and_withdrawal(&api, slashed_operator.0, slashed_operator.1)
        })
        .collect();

    let operator_nominators = join_all(fut_storages).await;
    for operator_nominator in operator_nominators {
        println!(
            "Operator[{:?}] has {:?} Nominators",
            operator_nominator.operator_id,
            operator_nominator.nominator_storage.len()
        )
    }
}

fn get_slashed_operators() -> Vec<(OperatorId, Number)> {
    vec![
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
    ]
}

async fn get_nominator_deposits_and_withdrawal(
    api: &Api,
    operator_id: OperatorId,
    slashed_block_number: Number,
) -> OperatorNominators {
    let deposits =
        get_nominator_storage::<Deposit>(api, operator_id, slashed_block_number, "Deposits").await;
    let withdrawals =
        get_nominator_storage::<Withdrawal>(api, operator_id, slashed_block_number, "Withdrawals")
            .await;
    let mut storage = BTreeMap::new();
    deposits.into_iter().for_each(|(nominator_id, deposit)| {
        storage.insert(
            nominator_id,
            NominatorStorage {
                deposit: Some(deposit),
                withdrawal: None,
            },
        );
    });
    withdrawals
        .into_iter()
        .for_each(|(nominator_id, withdrawal)| {
            let nominator_storage = match storage.get(&nominator_id) {
                None => NominatorStorage {
                    deposit: None,
                    withdrawal: Some(withdrawal),
                },
                Some(nominator_storage) => NominatorStorage {
                    deposit: nominator_storage.deposit.clone(),
                    withdrawal: None,
                },
            };
            storage.insert(nominator_id, nominator_storage);
        });
    let nominator_count = get_nominator_count(api, operator_id, slashed_block_number).await;
    assert_eq!(storage.len() as u32, nominator_count);
    OperatorNominators {
        operator_id,
        nominator_storage: storage,
    }
}

async fn get_nominator_storage<V: Decode>(
    api: &Api,
    operator_id: OperatorId,
    slashed_block_number: Number,
    storage: &'static str,
) -> Vec<(AccountId, V)> {
    let block_hash = api
        .get_block_hash(Some(slashed_block_number - 1))
        .await
        .ok()
        .flatten()
        .unwrap();
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

async fn get_nominator_count(
    api: &Api,
    operator_id: OperatorId,
    slashed_block_number: Number,
) -> u32 {
    let block_hash = api
        .get_block_hash(Some(slashed_block_number - 1))
        .await
        .ok()
        .flatten()
        .unwrap();
    let count = api
        .get_storage_map::<_, u32>("Domains", "NominatorCount", operator_id, Some(block_hash))
        .await
        .ok()
        .flatten()
        .unwrap();

    // + 1 since operator's nominator is not counted
    count + 1
}
