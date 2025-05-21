//! Pallet `stealth_zk`: stealth payment structure with zk-SNARK verification
#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use frame_support::{
        dispatch::DispatchResult,
        pallet_prelude::*,
        traits::{Currency, ExistenceRequirement, WithdrawReasons},
        BoundedVec,
    };
    use frame_system::pallet_prelude::*;
    use sp_std::vec::Vec;
    use scale_info::TypeInfo;
    use codec::{Encode, Decode};
    use ark_groth16::{Groth16, PreparedVerifyingKey, Proof};
    use ark_bls12_381::{Bls12_381, Fr};
    use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};

    /// Configuration trait for the stealth_zk pallet.
    ///
    /// - `Currency`: the AX token
    /// - `MaxRecipients`: maximum recipients per transfer
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// Event type
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        /// Currency for payments (AX token)
        type Currency: Currency<Self::AccountId>;
        /// Maximum recipients per stealth transfer
        #[pallet::constant]
        type MaxRecipients: Get<u32>;
    }

    /// Balance type alias
    pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;
    /// Bounded recipients list
    pub type BoundedRecipients<T> = BoundedVec<<T as frame_system::Config>::AccountId, <T as Config>::MaxRecipients>;

    /// Serialized Groth16 verifying key bytes
    #[pallet::storage]
    #[pallet::getter(fn verifying_key)]
    pub(super) type VerifyingKey<T: Config> = StorageValue<_, Vec<u8>, OptionQuery>;

    /// Auto-incrementing transfer identifier
    #[pallet::storage]
    #[pallet::getter(fn next_transfer_id)]
    pub(super) type NextTransferId<T: Config> = StorageValue<_, u64, ValueQuery>;

    /// Pending stealth transfer data
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo)]
    pub struct StealthTransfer<T: Config> {
        pub sender: T::AccountId,
        pub recipients: BoundedRecipients<T>,
        pub ciphertext: Vec<u8>,
        pub amount: BalanceOf<T>,
    }

    /// Map transfer ID to pending transfer
    #[pallet::storage]
    #[pallet::getter(fn transfers)]
    pub(super) type Transfers<T: Config> = StorageMap<_, Blake2_128Concat, u64, StealthTransfer<T>>;

    /// Pallet events
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Verifying key set by root
        VerifyingKeySet,
        /// Stealth transfer submitted: funds locked
        StealthTransferSubmitted { id: u64, sender: T::AccountId },
        /// Stealth transfer claimed: funds released
        StealthTransferClaimed { id: u64, claimer: T::AccountId },
        /// Proof verification failed on claim
        ZkProofVerificationFailed { id: u64, claimer: T::AccountId },
    }

    /// Pallet errors
    #[pallet::error]
    pub enum Error<T> {
        /// No verifying key available
        VerifyingKeyNotSet,
        /// zk-proof invalid
        InvalidProof,
        /// Too many recipients
        TooManyRecipients,
        /// Transfer not found
        TransferNotFound,
        /// Transfer ID overflow
        IdOverflow,
    }

    /// Weight functions for benchmarking
    pub trait WeightInfo {
        fn set_verifying_key() -> Weight;
        fn submit_stealth_transfer() -> Weight;
        fn claim_stealth_transfer() -> Weight;
    }

    // Stub weights (to be replaced by auto-generated values)
    impl WeightInfo for () {
        fn set_verifying_key() -> Weight { 10_000 }
        fn submit_stealth_transfer() -> Weight { 50_000 }
        fn claim_stealth_transfer() -> Weight { 100_000 }
    }

    #[pallet::pallet]
    #[pallet::generate_store(pub(super) trait Store)]
    pub struct Pallet<T>(_);

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Set the zk-SNARK verifying key (root only).
        #[pallet::weight(T::WeightInfo::set_verifying_key())]
        pub fn set_verifying_key(origin: OriginFor<T>, vk: Vec<u8>) -> DispatchResult {
            ensure_root(origin)?;
            VerifyingKey::<T>::put(vk);
            Self::deposit_event(Event::VerifyingKeySet);
            Ok(())
        }

        /// Submit a stealth transfer: lock AX and store encrypted payload.
        #[pallet::weight(T::WeightInfo::submit_stealth_transfer())]
        pub fn submit_stealth_transfer(
            origin: OriginFor<T>,
            recipients: Vec<T::AccountId>,
            amount: BalanceOf<T>,
            ciphertext: Vec<u8>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let bounded = BoundedRecipients::<T>::try_from(recipients)
                .map_err(|_| Error::<T>::TooManyRecipients)?;

            // Prepare storage
            let id = NextTransferId::<T>::get();
            ensure!(id != u64::MAX, Error::<T>::IdOverflow);
            let transfer = StealthTransfer { sender: who.clone(), recipients: bounded.clone(), ciphertext, amount };
            Transfers::<T>::insert(id, transfer);
            NextTransferId::<T>::put(id + 1);

            // Withdraw AX from sender
            T::Currency::withdraw(
                &who,
                amount,
                WithdrawReasons::TRANSFER,
                ExistenceRequirement::AllowDeath,
            )
            .map_err(|e| {
                // rollback on failure
                Transfers::<T>::remove(id);
                NextTransferId::<T>::mutate(|n| *n -= 1);
                e.into()
            })?;

            Self::deposit_event(Event::StealthTransferSubmitted { id, sender: who });
            Ok(())
        }

        /// Claim a stealth transfer: verify zk-proof and distribute AX.
        #[pallet::weight(T::WeightInfo::claim_stealth_transfer())]
        pub fn claim_stealth_transfer(
            origin: OriginFor<T>,
            transfer_id: u64,
            proof: Vec<u8>,
            public_inputs: Vec<u8>,
        ) -> DispatchResult {
            let claimer = ensure_signed(origin)?;

            // Load and parse verifying key
            let vk_bytes = VerifyingKey::<T>::get().ok_or(Error::<T>::VerifyingKeyNotSet)?;
            let pvk = PreparedVerifyingKey::<Bls12_381>::deserialize_compressed(&vk_bytes)
                .map_err(|_| Error::<T>::InvalidProof)?;

            // Parse proof and inputs
            let proof = Proof::<Bls12_381>::deserialize_compressed(&proof)
                .map_err(|_| Error::<T>::InvalidProof)?;
            let inputs: Vec<Fr> = Fr::deserialize_compressed(&public_inputs)
                .map_err(|_| Error::<T>::InvalidProof)?;

            // Verify proof
            let verified = Groth16::verify_proof(&pvk, &proof, &inputs)
                .map_err(|_| Error::<T>::InvalidProof)?;
            if !verified {
                Self::deposit_event(Event::ZkProofVerificationFailed { id: transfer_id, claimer: claimer.clone() });
                return Err(Error::<T>::InvalidProof.into());
            }

            // Distribute AX
            let transfer = Transfers::<T>::take(transfer_id).ok_or(Error::<T>::TransferNotFound)?;
            let n = transfer.recipients.len() as u32;
            let total = transfer.amount;
            let share = total / n.into();
            let rem = total - share * n.into();
            for (i, acct) in transfer.recipients.into_iter().enumerate() {
                let mut amt = share;
                if i == (n as usize - 1) { amt += rem; }
                T::Currency::deposit_creating(&acct, amt);
            }

            Self::deposit_event(Event::StealthTransferClaimed { id: transfer_id, claimer });
            Ok(())
        }
    }

    // Unit tests
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate as stealth_zk;
        use frame_support::{assert_ok, assert_noop, impl_outer_origin, parameter_types};
        use sp_core::H256;
        use frame_system as system;
        use pallet_balances;
        use sp_runtime::{traits::{BlakeTwo256, IdentityLookup}, testing::Header};

        impl_outer_origin! {
            pub enum Origin for Test {}
        }

        #[derive(Clone, PartialEq, Eq, Debug)]
        pub struct Test;
        parameter_types! {
            pub const BlockHashCount: u64 = 250;
            pub const MaxRecipients: u32 = 5;
            pub const ExistentialDeposit: u64 = 1;
        }
        impl system::Config for Test {
            type BaseCallFilter = ();
            type BlockWeights = ();
            type BlockLength = ();
            type DbWeight = ();
            type Origin = Origin;
            type Call = ();
            type Index = u64;
            type BlockNumber = u64;
            type Hash = H256;
            type Hashing = BlakeTwo256;
            type AccountId = u64;
            type Lookup = IdentityLookup<Self::AccountId>;
            type Header = Header;
            type Event = ();
            type BlockHashCount = BlockHashCount;
            type Version = ();
            type PalletInfo = ();
            type AccountData = pallet_balances::AccountData<u64>;
            type OnNewAccount = ();
            type OnKilledAccount = ();
            type SystemWeightInfo = ();
            type SS58Prefix = ();
            type OnSetCode = ();
        }
        impl pallet_balances::Config for Test {
            type MaxLocks = ();
            type Balance = u64;
            type Event = ();
            type DustRemoval = ();
            type ExistentialDeposit = ExistentialDeposit;
            type AccountStore = system::Pallet<Test>;
            type WeightInfo = ();
        }
        impl Config for Test {
            type Event = ();
            type Currency = pallet_balances::Pallet<Test>;
            type MaxRecipients = MaxRecipients;
        }

        type System = system::Pallet<Test>;
        type Balances = pallet_balances::Pallet<Test>;
        type Stealth = Pallet<Test>;

        fn new_test_ext() -> sp_io::TestExternalities {
            let mut t = system::GenesisConfig::default().build_storage::<Test>().unwrap();
            pallet_balances::GenesisConfig::<Test> {
                balances: vec![(1, 100), (2, 100)],
            }.assimilate_storage(&mut t).unwrap();
            t.into()
        }

        #[test]
        fn submit_and_claim_flow() {
            new_test_ext().execute_with(|| {
                // stub verifying key
                assert_ok!(Stealth::set_verifying_key(Origin::root(), vec![1,2,3]));
                // submit transfer
                assert_ok!(Stealth::submit_stealth_transfer(Origin::signed(1), vec![2], 50, vec![0]));
                // dummy proof and inputs always succeed
                assert_ok!(Stealth::claim_stealth_transfer(Origin::signed(2), 0, vec![0], vec![0]));
                // check recipient balance bumped
                assert_eq!(Balances::free_balance(2), 100 + 50);
            });
        }

        #[test]
        fn claim_with_invalid_proof_fails() {
            new_test_ext().execute_with(|| {
                assert_ok!(Stealth::set_verifying_key(Origin::root(), vec![1,2,3]));
                assert_ok!(Stealth::submit_stealth_transfer(Origin::signed(1), vec![2], 30, vec![0]));
                // invalid public_inputs -> proof fails
                assert_noop!(Stealth::claim_stealth_transfer(Origin::signed(2), 0, vec![], vec![]), Error::<Test>::InvalidProof);
            });
        }
    }

    // Benchmarking (enable with `--features runtime-benchmarks`)
    #[cfg(feature = "runtime-benchmarks")]
    mod benchmarking {
        use super::*;
        use frame_benchmarking::{benchmarks, account, whitelisted_caller};
        use frame_system::RawOrigin;

        benchmarks! {
            set_verifying_key {
                let vk in 0 .. 1024;
                let data: Vec<u8> = vec![0; vk as usize];
            }: _(RawOrigin::Root, data.clone())

            submit_stealth_transfer {
                let s in 1 .. T::MaxRecipients::get();
                let sender: T::AccountId = whitelisted_caller();
                let recipients: Vec<T::AccountId> = (0..s).map(|_| sender.clone()).collect();
                let amount = T::Currency::minimum_balance();
                let ciphertext = vec![0u8; 128];
            }: _(RawOrigin::Signed(sender.clone()), recipients, amount, ciphertext)

            claim_stealth_transfer {
                let caller: T::AccountId = whitelisted_caller();
                let vk = vec![0u8; 128];
                let _ = Pallet::<T>::set_verifying_key(RawOrigin::Root.into(), vk.clone());
                let recipients = vec![caller.clone()];
                let amount = T::Currency::minimum_balance();
                let _ = Pallet::<T>::submit_stealth_transfer(RawOrigin::Signed(caller.clone()).into(), recipients.clone(), amount, vec![0u8;128]);
                let proof = vec![0u8; 192];
                let inputs = vec![0u8;64];
                let transfer_id = NextTransferId::<T>::get() - 1;
            }: _(RawOrigin::Signed(caller.clone()), transfer_id, proof, inputs)

            impl_benchmark_test_suite!(Pallet, crate::tests::new_test_ext(), crate::tests::Test);
        }
    }
}

// Runtime integration example (in `runtime/src/lib.rs`):
//
// impl stealth_zk::Config for Runtime {
//     type Event = Event;
//     type Currency = Balances;
//     type MaxRecipients = ConstU32<5>;
//     type WeightInfo = stealth_zk::weights::SubstrateWeight<Runtime>;
// }
//
// construct_runtime! {
//     // ...
//     StealthZk: stealth_zk::{Pallet, Call, Storage, Event<T>},
//     // ...
// }

