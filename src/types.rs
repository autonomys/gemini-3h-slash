use crate::{AccountId, Balance, Number};
use codec::{Decode, Encode};
use scale_info::TypeInfo;
use sp_domains::{DomainId, EpochIndex, OperatorId, OperatorPublicKey};
use sp_runtime::traits::Zero;
use sp_runtime::{Perbill, Percent};
use std::collections::{BTreeMap, VecDeque};

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq, Default)]
pub(crate) struct Deposit {
    pub(crate) known: KnownDeposit,
    pub(crate) pending: Option<PendingDeposit>,
}

#[derive(TypeInfo, Debug, Encode, Decode, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct KnownDeposit {
    pub(crate) shares: Balance,
    pub(crate) storage_fee_deposit: Balance,
}

#[derive(TypeInfo, Debug, Encode, Decode, Copy, Clone, PartialEq, Eq)]
pub(crate) struct PendingDeposit {
    pub(crate) effective_domain_epoch: DomainEpoch,
    pub(crate) amount: Balance,
    pub(crate) storage_fee_deposit: Balance,
}

#[derive(TypeInfo, Debug, Encode, Decode, Copy, Clone, PartialEq, Eq)]
pub struct DomainEpoch(DomainId, EpochIndex);

impl DomainEpoch {
    pub(crate) fn deconstruct(&self) -> (DomainId, EpochIndex) {
        (self.0, self.1)
    }
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq, Default)]
pub(crate) struct Withdrawal {
    /// Total withdrawal amount requested by the nominator that are in unlocking state excluding withdrawal
    /// in shares and the storage fee
    pub(crate) total_withdrawal_amount: Balance,
    /// Individual withdrawal amounts with their unlocking block for a given domain
    pub(crate) withdrawals: VecDeque<WithdrawalInBalance>,
    /// Withdrawal that was initiated by nominator and not converted to balance due to
    /// unfinished domain epoch.
    pub(crate) withdrawal_in_shares: Option<WithdrawalInShares>,
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq)]
pub(crate) struct WithdrawalInBalance {
    pub(crate) domain_id: DomainId,
    pub(crate) unlock_at_confirmed_domain_block_number: Number,
    pub(crate) amount_to_unlock: Balance,
    pub(crate) storage_fee_refund: Balance,
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq)]
pub(crate) struct WithdrawalInShares {
    pub(crate) domain_epoch: DomainEpoch,
    pub(crate) unlock_at_confirmed_domain_block_number: Number,
    pub(crate) shares: Balance,
    pub(crate) storage_fee_refund: Balance,
}

pub(crate) struct NominatorStorage {
    pub(crate) deposit: Deposit,
    pub(crate) withdrawal: Option<Withdrawal>,
}

pub(crate) struct OperatorNominators {
    pub(crate) operator_id: OperatorId,
    pub(crate) nominator_storage: BTreeMap<AccountId, NominatorStorage>,
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq)]
pub struct OperatorDeregisteredInfo {
    pub domain_epoch: DomainEpoch,
    pub unlock_at_confirmed_domain_block_number: Number,
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq)]
pub enum OperatorStatus {
    Registered,
    /// De-registered at given domain epoch.
    Deregistered(OperatorDeregisteredInfo),
    Slashed,
    PendingSlash,
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq)]
pub struct Operator {
    pub signing_key: OperatorPublicKey,
    pub current_domain_id: DomainId,
    pub next_domain_id: DomainId,
    pub minimum_nominator_stake: Balance,
    pub nomination_tax: Percent,
    /// Total active stake of combined nominators under this operator.
    pub current_total_stake: Balance,
    /// Total rewards this operator received this current epoch.
    pub current_epoch_rewards: Balance,
    /// Total shares of all the nominators under this operator.
    pub current_total_shares: Balance,
    /// The status of the operator, it may be stale due to the `OperatorStatus::PendingSlash` is
    /// not assigned to this field directly, thus MUST use the `status()` method to query the status
    /// instead.
    /// TODO: update the filed to `_status` to avoid accidental access in next network reset
    status: OperatorStatus,
    /// Total deposits during the previous epoch
    pub deposits_in_epoch: Balance,
    /// Total withdrew shares during the previous epoch
    pub withdrawals_in_epoch: Balance,
    /// Total balance deposited to the bundle storage fund
    pub total_storage_fee_deposit: Balance,
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq, Default)]
pub struct SharePrice(Perbill);

impl SharePrice {
    /// Creates a new instance of share price from shares and stake.
    pub(crate) fn new(shares: Balance, stake: Balance) -> Self {
        SharePrice(if shares.is_zero() || stake.is_zero() {
            Perbill::one()
        } else {
            Perbill::from_rational(shares, stake.into())
        })
    }

    /// Converts stake to shares based on the share price
    pub(crate) fn stake_to_shares(&self, stake: Balance) -> Balance {
        if self.0.is_one() {
            stake.into()
        } else {
            self.0.mul_floor(stake).into()
        }
    }

    /// Converts shares to stake based on the share price
    pub(crate) fn shares_to_stake(&self, shares: Balance) -> Balance {
        if self.0.is_one() {
            shares.into()
        } else {
            self.0.saturating_reciprocal_mul_floor(shares.into())
        }
    }
}

#[derive(TypeInfo, Debug, Encode, Decode, Clone, PartialEq, Eq, Default)]
pub struct StorageFundRedeemPrice((Balance, Balance));

impl StorageFundRedeemPrice {
    pub(crate) fn new(total_balance: Balance, total_deposit: Balance) -> Self {
        StorageFundRedeemPrice((total_balance, total_deposit))
    }

    /// Return the amount of balance can be redeemed by the given `deposit`, it is calculated
    /// by `storage_fund_total_balance * deposit / total_deposit`.
    ///
    /// If the inflow of the storage fund (i.e. refund of the storage fee) is larger than its
    /// outflow (i.e. payment of the storage fee), the return value will larger than `deposit`
    /// otherwise smaller.
    pub(crate) fn redeem(&self, deposit: Balance) -> Balance {
        let (total_balance, total_deposit) = self.0;
        if total_balance == total_deposit {
            deposit
        } else {
            Perbill::from_rational(deposit, total_deposit).mul_floor(total_balance)
        }
    }
}
