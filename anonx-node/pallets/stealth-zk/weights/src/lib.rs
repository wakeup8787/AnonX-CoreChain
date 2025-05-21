#![cfg_attr(not(feature = "std"), no_std)]

//! Stub-crate definiujący kalkulacje weightów dla palety `stealth_zk`.
//! Po uruchomieniu benchmarków wygenerowany kod („benchmarking.rs”)
//! możesz wstawić tutaj, zastępując poniższy `SubstrateWeight`.

use frame_support::weights::Weight;
// Import traitu z Twojego paletu (zdefiniowany w lib.rs) :contentReference[oaicite:6]{index=6}
use stealth_zk::pallet::WeightInfo;

/// Domyślna implementacja stubbująca WeightInfo.
/// Zwraca proste, sztywne wartości – zastąp je automatycznie wygenerowanymi przez CLI.
pub struct SubstrateWeight;

impl WeightInfo for SubstrateWeight {
    fn set_verifying_key() -> Weight {
        // Domyślnie 10k
        10_000
    }

    fn submit_stealth_transfer() -> Weight {
        // Domyślnie 50k
        50_000
    }

    fn claim_stealth_transfer() -> Weight {
        // Domyślnie 100k
        100_000
    }
}

