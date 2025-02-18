use crate::blocks::Icrc1Block;
use crate::{Account, Block, Memo, Subaccount};
use candid::types::number::{Int, Nat};
use candid::CandidType;
use ic_base_types::CanisterId;
use ic_ledger_canister_core::ledger::TransferError as CoreTransferError;
use serde::Deserialize;
use serde_bytes::ByteBuf;
use std::convert::TryFrom;
use std::marker::PhantomData;

pub type NumTokens = Nat;
pub type BlockIndex = Nat;

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum TransferError {
    BadFee { expected_fee: NumTokens },
    BadBurn { min_burn_amount: NumTokens },
    InsufficientFunds { balance: NumTokens },
    TooOld,
    CreatedInFuture { ledger_time: u64 },
    TemporarilyUnavailable,
    Duplicate { duplicate_of: BlockIndex },
    GenericError { error_code: Nat, message: String },
}

impl From<CoreTransferError> for TransferError {
    fn from(err: CoreTransferError) -> Self {
        use ic_ledger_canister_core::ledger::TransferError as LTE;
        use TransferError as TE;

        match err {
            LTE::BadFee { expected_fee } => TE::BadFee {
                expected_fee: Nat::from(expected_fee.get_e8s()),
            },
            LTE::InsufficientFunds { balance } => TE::InsufficientFunds {
                balance: Nat::from(balance.get_e8s()),
            },
            LTE::TxTooOld { .. } => TE::TooOld,
            LTE::TxCreatedInFuture { ledger_time } => TE::CreatedInFuture {
                ledger_time: ledger_time.as_nanos_since_unix_epoch(),
            },
            LTE::TxThrottled => TE::TemporarilyUnavailable,
            LTE::TxDuplicate { duplicate_of } => TE::Duplicate {
                duplicate_of: Nat::from(duplicate_of),
            },
            LTE::InsufficientAllowance { .. } => todo!(),
            LTE::ExpiredApproval { .. } => todo!(),
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TransferArg {
    #[serde(default)]
    pub from_subaccount: Option<Subaccount>,
    pub to: Account,
    #[serde(default)]
    pub fee: Option<NumTokens>,
    #[serde(default)]
    pub created_at_time: Option<u64>,
    #[serde(default)]
    pub memo: Option<Memo>,
    pub amount: NumTokens,
}

/// Variant type for the `metadata` endpoint values.
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Nat(Nat),
    Int(Int),
    Text(String),
    Blob(ByteBuf),
}

impl Value {
    pub fn entry(key: impl ToString, val: impl Into<Value>) -> (String, Self) {
        (key.to_string(), val.into())
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(Int::from(n))
    }
}

impl From<i128> for Value {
    fn from(n: i128) -> Self {
        Value::Int(Int::from(n))
    }
}

impl From<u64> for Value {
    fn from(n: u64) -> Self {
        Value::Nat(Nat::from(n))
    }
}

impl From<u128> for Value {
    fn from(n: u128) -> Self {
        Value::Nat(Nat::from(n))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}

impl<'a> From<&'a str> for Value {
    fn from(s: &'a str) -> Self {
        Value::Text(s.to_string())
    }
}

impl From<Vec<u8>> for Value {
    fn from(bytes: Vec<u8>) -> Value {
        Value::Blob(ByteBuf::from(bytes))
    }
}

impl<'a> From<&'a [u8]> for Value {
    fn from(bytes: &'a [u8]) -> Value {
        Value::Blob(ByteBuf::from(bytes.to_vec()))
    }
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StandardRecord {
    pub name: String,
    pub url: String,
}

// Non-standard queries

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ArchiveInfo {
    pub canister_id: CanisterId,
    pub block_range_start: BlockIndex,
    pub block_range_end: BlockIndex,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GetTransactionsRequest {
    pub start: BlockIndex,
    pub length: Nat,
}

impl GetTransactionsRequest {
    pub fn as_start_and_length(&self) -> Result<(u64, u64), String> {
        use num_traits::cast::ToPrimitive;

        let start = self.start.0.to_u64().ok_or_else(|| {
            format!(
                "transaction index {} is too large, max allowed: {}",
                self.start,
                u64::MAX
            )
        })?;
        let length = self.length.0.to_u64().ok_or_else(|| {
            format!(
                "requested length {} is too large, max allowed: {}",
                self.length,
                u64::MAX
            )
        })?;
        Ok((start, length))
    }
}

pub type GetBlocksArgs = GetTransactionsRequest;

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Mint {
    pub amount: Nat,
    pub to: Account,
    pub memo: Option<Memo>,
    pub created_at_time: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Burn {
    pub amount: Nat,
    pub from: Account,
    pub memo: Option<Memo>,
    pub created_at_time: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub amount: Nat,
    pub from: Account,
    pub to: Account,
    pub memo: Option<Memo>,
    pub fee: Option<Nat>,
    pub created_at_time: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    pub kind: String,
    pub mint: Option<Mint>,
    pub burn: Option<Burn>,
    pub transfer: Option<Transfer>,
    pub timestamp: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ArchivedRange<Callback> {
    pub start: Nat,
    pub length: Nat,
    pub callback: Callback,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GetTransactionsResponse {
    pub log_length: Nat,
    pub first_index: Nat,
    pub transactions: Vec<Transaction>,
    pub archived_transactions: Vec<ArchivedRange<QueryTxArchiveFn>>,
}

#[derive(Debug, CandidType, Deserialize)]
pub struct GetBlocksResponse {
    pub first_index: BlockIndex,
    pub chain_length: u64,
    pub certificate: Option<serde_bytes::ByteBuf>,
    pub blocks: Vec<Icrc1Block>,
    pub archived_blocks: Vec<ArchivedRange<QueryBlockArchiveFn>>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TransactionRange {
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "candid::types::reference::Func")]
pub struct QueryArchiveFn<Input: CandidType, Output: CandidType> {
    pub canister_id: CanisterId,
    pub method: String,
    pub _marker: PhantomData<(Input, Output)>,
}

impl<Input: CandidType, Output: CandidType> QueryArchiveFn<Input, Output> {
    pub fn new(canister_id: CanisterId, method: impl Into<String>) -> Self {
        Self {
            canister_id,
            method: method.into(),
            _marker: PhantomData,
        }
    }
}

impl<Input: CandidType, Output: CandidType> Clone for QueryArchiveFn<Input, Output> {
    fn clone(&self) -> Self {
        Self {
            canister_id: self.canister_id,
            method: self.method.clone(),
            _marker: PhantomData,
        }
    }
}

impl<Input: CandidType, Output: CandidType> From<QueryArchiveFn<Input, Output>>
    for candid::types::reference::Func
{
    fn from(archive_fn: QueryArchiveFn<Input, Output>) -> Self {
        let p: &ic_base_types::PrincipalId = archive_fn.canister_id.as_ref();
        Self {
            principal: p.0,
            method: archive_fn.method,
        }
    }
}

impl<Input: CandidType, Output: CandidType> TryFrom<candid::types::reference::Func>
    for QueryArchiveFn<Input, Output>
{
    type Error = String;
    fn try_from(func: candid::types::reference::Func) -> Result<Self, Self::Error> {
        let canister_id = CanisterId::try_from(func.principal.as_slice())
            .map_err(|e| format!("principal is not a canister id: {}", e))?;
        Ok(QueryArchiveFn {
            canister_id,
            method: func.method,
            _marker: PhantomData,
        })
    }
}

impl<Input: CandidType, Output: CandidType> CandidType for QueryArchiveFn<Input, Output> {
    fn _ty() -> candid::types::Type {
        candid::types::Type::Func(candid::types::Function {
            modes: vec![candid::parser::types::FuncMode::Query],
            args: vec![Input::_ty()],
            rets: vec![Output::_ty()],
        })
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        candid::types::reference::Func::from(self.clone()).idl_serialize(serializer)
    }
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BlockRange {
    pub blocks: Vec<Icrc1Block>,
}

pub type QueryBlockArchiveFn = QueryArchiveFn<GetBlocksArgs, BlockRange>;
pub type QueryTxArchiveFn = QueryArchiveFn<GetTransactionsRequest, TransactionRange>;

impl From<Block> for Transaction {
    fn from(b: Block) -> Transaction {
        use crate::Operation;

        let mut tx = Transaction {
            kind: "".to_string(),
            mint: None,
            burn: None,
            transfer: None,
            timestamp: b.timestamp,
        };
        let created_at_time = b.transaction.created_at_time;
        let memo = b.transaction.memo;

        match b.transaction.operation {
            Operation::Mint { to, amount } => {
                tx.kind = "mint".to_string();
                tx.mint = Some(Mint {
                    to,
                    amount: Nat::from(amount),
                    created_at_time,
                    memo,
                });
            }
            Operation::Burn { from, amount } => {
                tx.kind = "burn".to_string();
                tx.burn = Some(Burn {
                    from,
                    amount: Nat::from(amount),
                    created_at_time,
                    memo,
                });
            }
            Operation::Transfer {
                from,
                to,
                amount,
                fee,
            } => {
                tx.kind = "transfer".to_string();
                tx.transfer = Some(Transfer {
                    from,
                    to,
                    amount: Nat::from(amount),
                    fee: fee
                        .map(Nat::from)
                        .or_else(|| b.effective_fee.map(Nat::from)),
                    created_at_time,
                    memo,
                });
            }
        }

        tx
    }
}
