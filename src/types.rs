use crate::{AccountId, Balance, Number};
use codec::{Decode, Encode};
use scale_info::TypeInfo;
use sp_domains::{DomainId, EpochIndex, OperatorId};
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
    pub(crate) deposit: Option<Deposit>,
    pub(crate) withdrawal: Option<Withdrawal>,
}

pub(crate) struct OperatorNominators {
    pub(crate) operator_id: OperatorId,
    pub(crate) nominator_storage: BTreeMap<AccountId, NominatorStorage>,
}
