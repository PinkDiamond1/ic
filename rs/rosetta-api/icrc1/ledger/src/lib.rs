pub mod cdk_runtime;

use crate::cdk_runtime::CdkRuntime;
use candid::{
    types::number::{Int, Nat},
    CandidType,
};
use ic_base_types::{CanisterId, PrincipalId};
use ic_icrc1::blocks::Icrc1Block;
use ic_icrc1::endpoints::{
    ArchivedRange, GetBlocksResponse, GetTransactionsResponse, QueryBlockArchiveFn,
    QueryTxArchiveFn, Transaction as Tx, Value,
};
use ic_icrc1::{Account, Block, LedgerBalances, Transaction};
use ic_ledger_canister_core::{
    archive::{ArchiveCanisterWasm, ArchiveOptions},
    blockchain::Blockchain,
    ledger::{apply_transaction, block_locations, LedgerContext, LedgerData, TransactionInfo},
    range_utils,
};
use ic_ledger_core::{
    approvals::AllowanceTable,
    balances::Balances,
    block::{BlockIndex, BlockType, EncodedBlock, HashOf},
    timestamp::TimeStamp,
    tokens::Tokens,
};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

const TRANSACTION_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_ACCOUNTS: usize = 28_000_000;
/// The maximum number of transactions the ledger should return for a single
/// get_transactions request.
const MAX_TRANSACTIONS_PER_REQUEST: usize = 2_000;
const ACCOUNTS_OVERFLOW_TRIM_QUANTITY: usize = 100_000;
const MAX_TRANSACTIONS_IN_WINDOW: usize = 3_000_000;
const MAX_TRANSACTIONS_TO_PURGE: usize = 100_000;

#[derive(Debug, Clone)]
pub struct Icrc1ArchiveWasm;

impl ArchiveCanisterWasm for Icrc1ArchiveWasm {
    fn archive_wasm() -> Cow<'static, [u8]> {
        Cow::Borrowed(include_bytes!(env!("IC_ICRC1_ARCHIVE_WASM_PATH")))
    }
}

/// Like [endpoints::Value], but can be serialized to CBOR.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum StoredValue {
    NatBytes(ByteBuf),
    IntBytes(ByteBuf),
    Text(String),
    Blob(ByteBuf),
}

impl From<StoredValue> for Value {
    fn from(v: StoredValue) -> Self {
        match v {
            StoredValue::NatBytes(num_bytes) => Self::Nat(
                Nat::decode(&mut &num_bytes[..])
                    .unwrap_or_else(|e| panic!("bug: invalid Nat encoding {:?}: {}", num_bytes, e)),
            ),
            StoredValue::IntBytes(int_bytes) => Self::Int(
                Int::decode(&mut &int_bytes[..])
                    .unwrap_or_else(|e| panic!("bug: invalid Int encoding {:?}: {}", int_bytes, e)),
            ),
            StoredValue::Text(text) => Self::Text(text),
            StoredValue::Blob(bytes) => Self::Blob(bytes),
        }
    }
}

impl From<Value> for StoredValue {
    fn from(v: Value) -> Self {
        match v {
            Value::Nat(num) => {
                let mut buf = vec![];
                num.encode(&mut buf).expect("bug: failed to encode nat");
                Self::NatBytes(ByteBuf::from(buf))
            }
            Value::Int(int) => {
                let mut buf = vec![];
                int.encode(&mut buf).expect("bug: failed to encode nat");
                Self::IntBytes(ByteBuf::from(buf))
            }
            Value::Text(text) => Self::Text(text),
            Value::Blob(bytes) => Self::Blob(bytes),
        }
    }
}

#[derive(Deserialize, CandidType, Clone, Debug, PartialEq, Eq)]
pub struct InitArgs {
    pub minting_account: Account,
    pub initial_balances: Vec<(Account, u64)>,
    pub transfer_fee: u64,
    pub token_name: String,
    pub token_symbol: String,
    pub metadata: Vec<(String, Value)>,
    pub archive_options: ArchiveOptions,
}

#[derive(Deserialize, CandidType, Clone, Debug, PartialEq, Eq)]
pub struct UpgradeArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Vec<(String, Value)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer_fee: Option<u64>,
}

#[derive(Deserialize, CandidType, Clone, Debug, PartialEq, Eq)]
pub enum LedgerArgument {
    Init(InitArgs),
    Upgrade(Option<UpgradeArgs>),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Ledger {
    balances: LedgerBalances,
    blockchain: Blockchain<CdkRuntime, Icrc1ArchiveWasm>,

    minting_account: Account,

    transactions_by_hash: BTreeMap<HashOf<Transaction>, BlockIndex>,
    transactions_by_height: VecDeque<TransactionInfo<Transaction>>,
    transfer_fee: Tokens,

    token_symbol: String,
    token_name: String,
    metadata: Vec<(String, StoredValue)>,
}

impl Ledger {
    pub fn from_init_args(
        InitArgs {
            minting_account,
            initial_balances,
            transfer_fee,
            token_name,
            token_symbol,
            metadata,
            archive_options,
        }: InitArgs,
        now: TimeStamp,
    ) -> Self {
        let mut ledger = Self {
            balances: LedgerBalances::default(),
            blockchain: Blockchain::new_with_archive(archive_options),
            transactions_by_hash: BTreeMap::new(),
            transactions_by_height: VecDeque::new(),
            minting_account,
            transfer_fee: Tokens::from_e8s(transfer_fee),
            token_symbol,
            token_name,
            metadata: metadata
                .into_iter()
                .map(|(k, v)| (k, StoredValue::from(v)))
                .collect(),
        };

        for (account, balance) in initial_balances.into_iter() {
            apply_transaction(
                &mut ledger,
                Transaction::mint(account, Tokens::from_e8s(balance), Some(now), None),
                now,
                Tokens::ZERO,
            )
            .unwrap_or_else(|err| {
                panic!("failed to mint {} e8s to {}: {:?}", balance, account, err)
            });
        }

        ledger
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ApprovalKey(Account, PrincipalId);

impl From<(&Account, &PrincipalId)> for ApprovalKey {
    fn from((account, principal): (&Account, &PrincipalId)) -> Self {
        Self(*account, *principal)
    }
}

impl LedgerContext for Ledger {
    type AccountId = Account;
    type SpenderId = PrincipalId;
    type Approvals = AllowanceTable<ApprovalKey, Account, PrincipalId>;
    type BalancesStore = HashMap<Self::AccountId, Tokens>;

    fn balances(&self) -> &Balances<Self::BalancesStore> {
        &self.balances
    }

    fn balances_mut(&mut self) -> &mut Balances<Self::BalancesStore> {
        &mut self.balances
    }

    fn approvals(&self) -> &Self::Approvals {
        unimplemented!()
    }

    fn approvals_mut(&mut self) -> &mut Self::Approvals {
        unimplemented!()
    }
}

impl LedgerData for Ledger {
    type Runtime = CdkRuntime;
    type ArchiveWasm = Icrc1ArchiveWasm;
    type Transaction = Transaction;
    type Block = Block;

    fn transaction_window(&self) -> Duration {
        TRANSACTION_WINDOW
    }

    fn max_transactions_in_window(&self) -> usize {
        MAX_TRANSACTIONS_IN_WINDOW
    }

    fn max_transactions_to_purge(&self) -> usize {
        MAX_TRANSACTIONS_TO_PURGE
    }

    fn max_number_of_accounts(&self) -> usize {
        MAX_ACCOUNTS
    }

    fn accounts_overflow_trim_quantity(&self) -> usize {
        ACCOUNTS_OVERFLOW_TRIM_QUANTITY
    }

    fn token_name(&self) -> &str {
        &self.token_name
    }

    fn token_symbol(&self) -> &str {
        &self.token_symbol
    }

    fn blockchain(&self) -> &Blockchain<Self::Runtime, Self::ArchiveWasm> {
        &self.blockchain
    }

    fn blockchain_mut(&mut self) -> &mut Blockchain<Self::Runtime, Self::ArchiveWasm> {
        &mut self.blockchain
    }

    fn transactions_by_hash(&self) -> &BTreeMap<HashOf<Self::Transaction>, BlockIndex> {
        &self.transactions_by_hash
    }

    fn transactions_by_hash_mut(&mut self) -> &mut BTreeMap<HashOf<Self::Transaction>, BlockIndex> {
        &mut self.transactions_by_hash
    }

    fn transactions_by_height(&self) -> &VecDeque<TransactionInfo<Self::Transaction>> {
        &self.transactions_by_height
    }

    fn transactions_by_height_mut(&mut self) -> &mut VecDeque<TransactionInfo<Self::Transaction>> {
        &mut self.transactions_by_height
    }

    fn on_purged_transaction(&mut self, _height: BlockIndex) {}
}

impl Ledger {
    pub fn minting_account(&self) -> &Account {
        &self.minting_account
    }

    pub fn transfer_fee(&self) -> Tokens {
        self.transfer_fee
    }

    pub fn metadata(&self) -> Vec<(String, Value)> {
        let mut records: Vec<(String, Value)> = self
            .metadata
            .clone()
            .into_iter()
            .map(|(k, v)| (k, StoredValue::into(v)))
            .collect();
        let decimals = ic_ledger_core::tokens::DECIMAL_PLACES as u64;
        records.push(Value::entry("icrc1:decimals", decimals));
        records.push(Value::entry("icrc1:name", self.token_name()));
        records.push(Value::entry("icrc1:symbol", self.token_symbol()));
        records.push(Value::entry("icrc1:fee", self.transfer_fee().get_e8s()));
        records
    }

    pub fn upgrade_metadata(&mut self, args: UpgradeArgs) {
        if let Some(upgrade_metadata_args) = args.metadata {
            self.metadata = upgrade_metadata_args
                .into_iter()
                .map(|(k, v)| (k, StoredValue::from(v)))
                .collect();
        }
        if let Some(token_name) = args.token_name {
            self.token_name = token_name;
        }
        if let Some(token_symbol) = args.token_symbol {
            self.token_symbol = token_symbol;
        }
        if let Some(transfer_fee) = args.transfer_fee {
            self.transfer_fee = Tokens::from_e8s(transfer_fee);
        }
    }

    /// Returns the root hash of the certified ledger state.
    /// The canister code must call set_certified_data with the value this function returns after
    /// each successful modification of the ledger.
    pub fn root_hash(&self) -> [u8; 32] {
        use ic_crypto_tree_hash::{Label, MixedHashTree as T};
        let tree = match self.blockchain().last_hash {
            Some(hash) => T::Labeled(
                Label::from("tip_hash"),
                Box::new(T::Leaf(hash.as_slice().to_vec())),
            ),
            None => T::Empty,
        };
        tree.digest().0
    }

    fn query_blocks<ArchiveFn, B>(
        &self,
        start: BlockIndex,
        length: usize,
        decode: impl Fn(&EncodedBlock) -> B,
        make_callback: impl Fn(CanisterId) -> ArchiveFn,
    ) -> (u64, Vec<B>, Vec<ArchivedRange<ArchiveFn>>) {
        let locations = block_locations(self, start, length);

        let local_blocks_range =
            range_utils::take(&locations.local_blocks, MAX_TRANSACTIONS_PER_REQUEST);

        let local_blocks: Vec<B> = self
            .blockchain
            .block_slice(local_blocks_range)
            .iter()
            .map(decode)
            .collect();

        let archived_blocks = locations
            .archived_blocks
            .into_iter()
            .map(|(canister_id, slice)| ArchivedRange {
                start: Nat::from(slice.start),
                length: Nat::from(range_utils::range_len(&slice)),
                callback: make_callback(canister_id),
            })
            .collect();

        (locations.local_blocks.start, local_blocks, archived_blocks)
    }

    /// Returns transactions in the specified range.
    pub fn get_transactions(&self, start: BlockIndex, length: usize) -> GetTransactionsResponse {
        let (first_index, local_transactions, archived_transactions) = self.query_blocks(
            start,
            length,
            |enc_block| -> Tx {
                Block::decode(enc_block.clone())
                    .expect("bug: failed to decode encoded block")
                    .into()
            },
            |canister_id| QueryTxArchiveFn::new(canister_id, "get_transactions"),
        );

        GetTransactionsResponse {
            first_index: Nat::from(first_index),
            log_length: Nat::from(self.blockchain.chain_length()),
            transactions: local_transactions,
            archived_transactions,
        }
    }

    /// Returns blocks in the specified range.
    pub fn get_blocks(&self, start: BlockIndex, length: usize) -> GetBlocksResponse {
        let (first_index, local_blocks, archived_blocks) = self.query_blocks(
            start,
            length,
            |enc_block| Icrc1Block::from(enc_block),
            |canister_id| QueryBlockArchiveFn::new(canister_id, "get_blocks"),
        );

        GetBlocksResponse {
            first_index: Nat::from(first_index),
            chain_length: self.blockchain.chain_length(),
            certificate: ic_cdk::api::data_certificate().map(serde_bytes::ByteBuf::from),
            blocks: local_blocks,
            archived_blocks,
        }
    }
}
