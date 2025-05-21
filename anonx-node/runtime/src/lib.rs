#![cfg_attr(not(feature = "std"), no_std)]

// Publiczny eksport palety
pub use stealth_zk;

#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

pub mod apis;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarks;
pub mod configs;

extern crate alloc;
use alloc::vec::Vec;

use sp_runtime::{
    generic, impl_opaque_keys,
    traits::{BlakeTwo256, IdentifyAccount, Verify},
    MultiAddress, MultiSignature,
};

#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

use frame_support::{create_runtime_str, traits::ConstU32};
use apis::RUNTIME_API_VERSIONS;

use stealth_zk::pallet as stealth_zk;
use stealth_zk_weights::SubstrateWeight;

use pallet_aura::Aura;
use pallet_grandpa::Grandpa;

pub use frame_system::Call as SystemCall;
pub use pallet_balances::Call as BalancesCall;
pub use pallet_timestamp::Call as TimestampCall;
pub use stealth_zk::Call as StealthZkCall;
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;

pub mod genesis_config_presets;

/// Opaque types dla wewnętrznej pracy node’a.
pub mod opaque {
    use super::*;
    use sp_runtime::{generic, traits::{BlakeTwo256, Hash as HashT}};

    pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;
    pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
    pub type Block = generic::Block<Header, UncheckedExtrinsic>;
    pub type BlockId = generic::BlockId<Block>;
    pub type Hash = <BlakeTwo256 as HashT>::Output;
}

impl_opaque_keys! {
    pub struct SessionKeys {
        pub aura: Aura,
        pub grandpa: Grandpa,
    }
}

#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
    spec_name:         create_runtime_str!("anonx-runtime"),
    impl_name:         create_runtime_str!("anonx-runtime"),
    authoring_version: 1,
    spec_version:      100,
    impl_version:      1,
    apis:              RUNTIME_API_VERSIONS,
    transaction_version: 1,
};

#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
    NativeVersion { runtime_version: VERSION.clone(), can_author_with: Default::default() }
}

mod block_times {
    pub const MILLISECS_PER_BLOCK: u64 = 6000;
    pub const SLOT_DURATION: u64 = MILLISECS_PER_BLOCK;
}
pub use block_times::*;

pub const MINUTES: BlockNumber = 60_000 / (MILLISECS_PER_BLOCK as BlockNumber);
pub const HOURS: BlockNumber = MINUTES * 60;
pub const DAYS: BlockNumber = HOURS * 24;

pub const BLOCK_HASH_COUNT: BlockNumber = 2400;

pub const UNIT: Balance = 1_000_000_000_000;
pub const MILLI_UNIT: Balance = 1_000_000_000;
pub const MICRO_UNIT: Balance = 1_000_000;

pub const EXISTENTIAL_DEPOSIT: Balance = MILLI_UNIT;

pub type Signature = MultiSignature;
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;
pub type Balance = u128;
pub type Nonce = u32;
pub type Hash = sp_core::H256;
pub type BlockNumber = u32;
pub type Address = MultiAddress<AccountId, ()>;
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
pub type Block = generic::Block<Header, UncheckedExtrinsic>;
pub type SignedBlock = generic::SignedBlock<Block>;
pub type BlockId = generic::BlockId<Block>;
pub type TxExtension = (
    frame_system::CheckSpecVersion<Runtime>,
    frame_system::CheckTxVersion<Runtime>,
    frame_system::CheckGenesis<Runtime>,
    frame_system::CheckEra<Runtime>,
    frame_system::CheckNonce<Runtime>,
    frame_system::CheckWeight<Runtime>,
    pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
    frame_metadata_hash_extension::CheckMetadataHash<Runtime>,
);
pub type UncheckedExtrinsic = generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;
pub type SignedPayload = generic::SignedPayload<RuntimeCall, TxExtension>;

type Migrations = ();

pub type Executive = frame_executive::Executive<
    Runtime,
    Block,
    frame_system::ChainContext<Runtime>,
    Runtime,
    AllPalletsWithSystem,
    Migrations,
>;

impl stealth_zk::Config for Runtime {
    type Event         = RuntimeEvent;
    type Currency      = Balances;
    type MaxRecipients = ConstU32<5>;
    type WeightInfo    = SubstrateWeight;
}

/// Wszystkie palety z systemem w jednej krotce, wykorzystywane przez `Executive`
type AllPalletsWithSystem = (
    System,
    Timestamp,
    Aura,
    Grandpa,
    Balances,
    TransactionPayment,
    Sudo,
    Template,
    StealthZk,
);

#[frame_support::runtime]
mod runtime {
    #[runtime::runtime]
    #[runtime::derive(
        RuntimeCall,
        RuntimeEvent,
        RuntimeError,
        RuntimeOrigin,
        RuntimeFreezeReason,
        RuntimeHoldReason,
        RuntimeSlashReason,
        RuntimeLockId,
        RuntimeTask,
        RuntimeViewFunction
    )]
    pub struct Runtime;

    #[runtime::pallet_index(0)]
    pub type System             = frame_system;
    #[runtime::pallet_index(1)]
    pub type Timestamp          = pallet_timestamp;
    #[runtime::pallet_index(2)]
    pub type Aura               = pallet_aura;
    #[runtime::pallet_index(3)]
    pub type Grandpa            = pallet_grandpa;
    #[runtime::pallet_index(4)]
    pub type Balances           = pallet_balances;
    #[runtime::pallet_index(5)]
    pub type TransactionPayment = pallet_transaction_payment;
    #[runtime::pallet_index(6)]
    pub type Sudo               = pallet_sudo;
    #[runtime::pallet_index(7)]
    pub type Template           = pallet_template;
    #[runtime::pallet_index(8)]
    pub type StealthZk          = stealth_zk;
}

